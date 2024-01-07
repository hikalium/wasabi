extern crate alloc;

use crate::boot_info::File;
use crate::elf;
use crate::elf::SectionHeader;
use crate::elf::SegmentHeader;
use crate::error::Error;
use crate::error::Result;
use crate::memory::AddressRange;
use crate::memory::ContiguousPhysicalMemoryPages;
use crate::println;
use crate::util::read_le_u16;
use crate::util::read_le_u32;
use crate::util::read_le_u64;
use crate::util::write_le_u64;
use crate::x86_64::paging::PageAttr;
use crate::x86_64::ExecutionContext;
use crate::x86_64::CONTEXT_OS;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::arch::asm;
use core::cmp::max;
use core::cmp::min;
use core::fmt;
use core::mem::size_of;

pub struct LoadedElf<'a> {
    elf: &'a Elf<'a>,
    region: ContiguousPhysicalMemoryPages,
    app_vaddr_range: AddressRange,
    loaded_segments: Vec<&'a elf::SegmentHeader>,
}
impl<'a> LoadedElf<'a> {
    fn resolve_vaddr(&self, vaddr: usize) -> Result<usize> {
        for s in &self.loaded_segments {
            if s.vaddr_range().contains(vaddr) {
                return Ok(self.region.range().start() + self.app_vaddr_range.offset_of(vaddr)?);
            }
        }
        Err(Error::Failed("vaddr not found"))
    }
    pub fn exec(self) -> Result<i64> {
        let stack_size = 8 * 1024;
        let mut stack = ContiguousPhysicalMemoryPages::alloc_bytes(stack_size)?;
        let stack_range = stack.range();
        stack.fill_with_bytes(0);
        stack.set_page_attr(PageAttr::ReadWriteUser)?;
        let entry_point = self.resolve_vaddr(self.elf.entry_vaddr as usize)?;
        let os_ctx = ExecutionContext::allocate();
        {
            let mut ctx = CONTEXT_OS.lock();
            *ctx = os_ctx;
        }
        let retcode: i64;
        unsafe {
            asm!(
                // Save current execution state in os_ctx
                // General registers
                "xchg rsp,rsi", // swap rsi with rsp to utilize push/pop
                "push rsi", // ExecutionContext.rsp
                "push r15",
                "push r14",
                "push r13",
                "push r12",
                "push r11",
                "push r10",
                "push r9",
                "push r8",
                "push rdi",
                "push rsi",
                "push rbp",
                "push rbx",
                "push rdx",
                "push rcx",
                "push rax",
                "pushfq", // ExecutionContext.rflags
                "lea r8, [rip+0f]", // ExecutionContext.rip
                "push r8", // ExecutionContext.rip
                "sub rsp, 512",
                "fxsave64[rsp]",
                "xchg rsp,rsi", // recover the original rsp

                // Set data segments to USER_DS
                "mov es, dx", // SS = crate::x86_64::USER_DS
                "mov ds, dx", // SS = crate::
                "mov fs, dx",
                "mov gs, dx",
                // Use iretq to switch to user mode
                "push rdx", // SS = crate::x86_64::USER_DS
                "push rax", // RSP = stack_range.end()
                "mov eax, 2",
                "push rax", // RFLAGS = 2
                "push rcx", // CS = crate::x86_64::USER64_CS
                "push rdi", // RIP = entry_point
                // *(rip as *const InterruptContext) == {
                //   rip: u64,
                //   cs: u64,
                //   rflags: u64,
                //   rsp: u64,
                //   ss: u64,
                // }
                // far-jmp to app using ireq
                "iretq",

                // **** exit from app ****
                "0:",
                // rdi (first arg in systemv abi) should be a pointer of ExecutionContext
                // See crate::syscall::sys_exit
                "mov rsp, rdi",
                "mov di,ss",
                "mov ds,di",
                "mov es,di",
                "mov fs,di",
                "mov gs,di",
                "fxrstor64[rsp]",
                "add rsp, 512",
                "pop rax", // drop ExecutionContext.rip
                "pop rax", // drop ExecutionContext.rflags
                "pop rax",
                "pop rcx",
                "pop rdx",
                "pop rbx",
                "pop rbp",
                "pop rsi",
                "pop rdi",
                "pop r8",
                "pop r9",
                "pop r10",
                "pop r11",
                "pop r12",
                "pop r13",
                "pop r14",
                "pop r15",
                "pop rsp", // ExecutionContext.rsp
                in("rax") stack_range.end(), // stack grows toward 0, so empty stack pointer will be the end addr
                in("rcx") crate::x86_64::USER64_CS,
                in("rdx") crate::x86_64::USER_DS,
                // rbx is used for LLVM internally
                in("rsi") (os_ctx as *mut u8).add(size_of::<ExecutionContext>()),
                in("rdi") entry_point,
                lateout("rax") retcode,
            );
            println!("returned from the code! retcode = {}", retcode);
        }
        Ok(retcode)
    }
    pub fn slice_of_vaddr_range(&self, range_on_vaddr: AddressRange) -> Result<&[u8]> {
        let range = range_on_vaddr.into_range_in(&self.app_vaddr_range)?;
        Ok(&self.region.as_slice()[range])
    }
    pub fn mut_slice_of_vaddr_range(&mut self, range_on_vaddr: AddressRange) -> Result<&mut [u8]> {
        let range = range_on_vaddr.into_range_in(&self.app_vaddr_range)?;
        Ok(&mut self.region.as_mut_slice()[range])
    }
    pub fn write_le_u64_at_vaddr(&mut self, vaddr: usize, value: u64) -> Result<()> {
        let bytes = self
            .mut_slice_of_vaddr_range(AddressRange::from_start_and_size(vaddr, size_of::<u64>()))?;
        write_le_u64(bytes, 0, value)
    }
}

pub struct Elf<'a> {
    file: &'a File,
    entry_vaddr: u64,
    segments: Vec<SegmentHeader>,
    _sections: BTreeMap<String, SectionHeader>,
    string_table: Option<&'a [u8]>,
}
impl<'a> Elf<'a> {
    fn read_string_from_table(string_table: &Option<&[u8]>, name_ofs: usize) -> String {
        if let Some(string_table) = string_table {
            if name_ofs >= string_table.len() {
                "(out of range)".to_string()
            } else {
                let section_name = &string_table[name_ofs..];
                let section_name: Vec<u8> = section_name
                    .iter()
                    .cloned()
                    .take_while(|c| *c != 0)
                    .collect();
                core::str::from_utf8(&section_name)
                    .unwrap_or("(invalid)")
                    .to_string()
            }
        } else {
            "(no string table)".to_string()
        }
    }
    pub fn read_string(&self, name_ofs: usize) -> String {
        Self::read_string_from_table(&self.string_table, name_ofs)
    }
    pub fn parse(file: &'a File) -> Result<Self> {
        let data = file.data();
        // https://wiki.osdev.org/ELF#Header
        if &data[0..4] != b"\x7fELF".as_slice() {
            return Err(Error::Failed("No ELF signature found"));
        }
        if data[4] != 2 /* ET_EXEC */ && data[4] != 3
        /*ET_DYN*/
        {
            return Err(Error::Failed("Not a 64-bit ELF"));
        }
        if data[5] != 1 {
            return Err(Error::Failed("Not a litte endian ELF"));
        }
        if data[7] != 0 {
            return Err(Error::Failed("ABI is not SystemV"));
        }
        let elf_type = read_le_u16(data, 16)?;
        if elf_type != 2 /* ET_EXEC */ && elf_type != 3
        /*ET_DYN*/
        {
            return Err(Error::Failed("Not an executable ELF"));
        }
        if read_le_u16(data, 18)? != 0x3E {
            return Err(Error::Failed("Not an x86_64 ELF"));
        }

        let entry_vaddr = read_le_u64(data, 24)?;

        let phdr_start = read_le_u64(data, 32)?;
        let phdr_entry_size = read_le_u16(data, 54)?;
        let num_of_phdr_entry = read_le_u16(data, 56)?;

        let shdr_start = read_le_u64(data, 40)?;
        let shdr_entry_size = read_le_u16(data, 58)?;
        let num_of_shdr_entry = read_le_u16(data, 60)?;

        let shdr_name_table_idx = read_le_u16(data, 62)?;

        let phdr_indexes = 0..num_of_phdr_entry;
        let mut segments = Vec::new();
        for i in phdr_indexes {
            let ofs = (phdr_start + i as u64 * phdr_entry_size as u64) as usize;
            let phdr_entry = &data[ofs..(ofs + phdr_entry_size as usize)];
            let phdr_type = read_le_u32(phdr_entry, 0)?;
            let entry_type = read_le_u32(phdr_entry, 4)?;
            let offset = read_le_u64(phdr_entry, 8)?;
            let vaddr = read_le_u64(phdr_entry, 16)?;
            let fsize = read_le_u64(phdr_entry, 32)?;
            let vsize = read_le_u64(phdr_entry, 40)?;
            let align = read_le_u64(phdr_entry, 48)?;
            segments.push(SegmentHeader {
                phdr_type,
                entry_type,
                offset,
                vaddr,
                fsize,
                vsize,
                align,
            });
        }

        let shdr_indexes = 0..num_of_shdr_entry;
        let mut sections = Vec::new();
        for i in shdr_indexes {
            let ofs = (shdr_start + i as u64 * shdr_entry_size as u64) as usize;
            let shdr_entry = &data[ofs..(ofs + shdr_entry_size as usize)];
            if shdr_entry.len() < size_of::<SectionHeader>() {
                return Err(Error::Failed("Section size is invalid"));
            }
            let section = shdr_entry.as_ptr() as *const SectionHeader;
            // SAFETY: This is safe since the check above ensures the bounds
            let section = unsafe { *section };
            sections.push(section);
        }
        let shdr_name_table = sections[shdr_name_table_idx as usize];
        let shdr_name_table = &data[shdr_name_table.offset as usize
            ..(shdr_name_table.offset + shdr_name_table.size) as usize];
        let sections: BTreeMap<String, SectionHeader> = sections
            .iter()
            .map(|s| {
                (
                    Self::read_string_from_table(&Some(shdr_name_table), s.name_ofs as usize),
                    *s,
                )
            })
            .collect();
        let string_table = sections
            .get(".strtab")
            .and_then(|s| s.file_range().ok())
            .map(|r| &data[r]);
        Ok(Self {
            file,
            entry_vaddr,
            segments,
            _sections: sections,
            string_table,
        })
    }
    /// Load a segment to the region
    fn load_segment(
        &self,
        region: &mut ContiguousPhysicalMemoryPages,
        app_vaddr_range: &AddressRange,
        sh: &elf::SegmentHeader,
    ) -> Result<()> {
        let segment_vaddr_range = sh.vaddr_range();
        let segment_file_range = sh.file_range();
        println!("Loading Segment: {segment_vaddr_range:?}...");

        let dst = region.as_mut_slice();
        let src = self.file.data();

        let dst = &mut dst[sh.vaddr_range().into_range_in(app_vaddr_range)?];
        let src = &src[segment_file_range];
        dst[..src.len()].copy_from_slice(src);

        Ok(())
    }
    pub fn load(&self) -> Result<LoadedElf> {
        let segments_to_be_loaded: Vec<&elf::SegmentHeader> = self
            .segments
            .iter()
            .filter(|s| s.phdr_type == elf::PHDR_TYPE_LOAD)
            .collect();
        if segments_to_be_loaded.is_empty() {
            return Err(Error::Failed("LOAD segment not found"));
        }
        let app_vaddr_range: AddressRange = AddressRange::from(
            segments_to_be_loaded
                .iter()
                .map(|s| {
                    (
                        s.vaddr & !(s.align - 1),
                        (s.vaddr + s.vsize).wrapping_add(s.align - 1) & !(s.align - 1),
                    )
                })
                .fold((u64::MAX, u64::MIN), |l, r| (min(l.0, r.0), max(l.1, r.1))),
        );
        println!("App_vaddr_range: {app_vaddr_range:?}");
        let mut region = ContiguousPhysicalMemoryPages::alloc_bytes(app_vaddr_range.size())?;
        region.fill_with_bytes(0);
        let region_range = region.range();
        println!("App region allocated = {region_range:?}",);
        region.set_page_attr(PageAttr::ReadWriteUser)?;
        for s in &segments_to_be_loaded {
            self.load_segment(&mut region, &app_vaddr_range, s)?;
        }
        let loaded_segments = segments_to_be_loaded;
        let mut loaded = LoadedElf {
            elf: self,
            region,
            app_vaddr_range,
            loaded_segments,
        };

        let dynamic_segment: Option<&elf::SegmentHeader> = self
            .segments
            .iter()
            .find(|s| s.phdr_type == elf::PHDR_TYPE_DYNAMIC);
        if let Some(dynamic_segment) = dynamic_segment {
            println!("DYNAMIC segment found");
            let frange = dynamic_segment.offset as usize
                ..(dynamic_segment.offset + dynamic_segment.fsize) as usize;
            let entries = &self.file.data()[frange]
                .chunks_exact(size_of::<elf::DynamicEntry>())
                .map(elf::DynamicEntry::try_from)
                .collect::<Result<Vec<elf::DynamicEntry>>>()?;
            for e in entries {
                println!("{:?}", e);
            }
            let rela_addr = entries
                .iter()
                .find(|e| e.tag == elf::DYNAMIC_TAG_RELA_ADDRESS)
                .map(|e| e.value as usize);
            let rela_total_size = entries
                .iter()
                .find(|e| e.tag == elf::DYNAMIC_TAG_RELA_TOTAL_SIZE)
                .map(|e| e.value as usize);
            let rela_entry_size = entries
                .iter()
                .find(|e| e.tag == elf::DYNAMIC_TAG_RELA_ENTRY_SIZE)
                .map(|e| e.value as usize);
            if let (Some(rela_addr), Some(rela_total_size), Some(rela_entry_size)) =
                (rela_addr, rela_total_size, rela_entry_size)
            {
                println!("RELA found. addr = {rela_addr:#018X} size = {rela_total_size:#018X}");
                let rela_data = loaded.slice_of_vaddr_range(AddressRange::from_start_and_size(
                    rela_addr,
                    rela_total_size,
                ))?;
                let rela_entries = &rela_data
                    .chunks_exact(rela_entry_size)
                    .map(elf::RelocationEntry::try_from)
                    .collect::<Result<Vec<elf::RelocationEntry>>>()?;
                for e in rela_entries {
                    let rel_type = e.info & 0xffffffff;
                    if rel_type == elf::R_386_RELATIVE {
                        let resolved = loaded.resolve_vaddr(e.addend as usize)?;
                        loaded.write_le_u64_at_vaddr(e.address as usize, resolved as u64)?;
                    } else {
                        return Err(Error::FailedString(format!(
                            "Relocation type {rel_type} is not supported yet"
                        )));
                    }
                }
            }
        };

        Ok(loaded)
    }
}
impl<'a> fmt::Debug for Elf<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Elf {{ name: {}, data: @{:#p} }}",
            &self.file.name(),
            self.file.data().as_ptr()
        )
    }
}
