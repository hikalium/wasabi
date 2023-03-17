extern crate alloc;

use crate::boot_info::File;
use crate::error::Error;
use crate::error::Result;
use crate::memory::ContiguousPhysicalMemoryPages;
use crate::print;
use crate::println;
use crate::util::read_le_u16;
use crate::util::read_le_u32;
use crate::util::read_le_u64;
use crate::util::write_le_u64;
use crate::util::PAGE_SIZE;
use crate::x86_64::paging::with_current_page_table;
use crate::x86_64::paging::PageAttr;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::arch::asm;
use core::fmt;
use core::mem::size_of;
use core::ops::Range;

const PHDR_TYPE_LOAD: u32 = 1;

#[derive(Copy, Clone)]
#[allow(unused)]
#[repr(C)]
pub struct SegmentHeader {
    phdr_type: u32,
    entry_type: u32,
    offset: u64,
    vaddr: u64,
    fsize: u64,
    vsize: u64,
    align: u64,
}
impl fmt::Debug for SegmentHeader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "type: {:#010X}", self.phdr_type)?;
        write!(f, ", attr(rwx): ({:03b})", self.entry_type)?; // RWX
        write!(f, ", offset: {:#018X}", self.offset)?;
        write!(f, ", vaddr: {:#018X}", self.vaddr)?;
        write!(f, ", fsize: {:#018X}", self.fsize)?;
        write!(f, ", vsize: {:#018X}", self.vsize)?;
        write!(f, ", align: {:#018X}", self.align)?;
        Ok(())
    }
}

pub struct LoadedSegment {
    region: ContiguousPhysicalMemoryPages,
    range_in_region: Range<usize>,
    range_vaddr: Range<usize>,
    range_vaddr_relocated: Range<usize>,
}
impl LoadedSegment {
    /// Load a segment with a specified index to the memory
    fn new(elf: &Elf, sh_index: usize) -> Result<Self> {
        println!("Loading Segment #{}...", sh_index);

        let sh = elf
            .segments
            .get(sh_index)
            .ok_or(Error::Failed("sh_index out of range"))?;

        assert_eq!(PAGE_SIZE % sh.align as usize, 0);
        let align_mask = sh.align as usize - 1;

        let range_file = {
            let start = sh.offset as usize;
            let end = start + sh.fsize as usize;
            println!("range in file          : {start:#018X}-{end:#018X}");
            start..end
        };
        let range_vaddr = {
            let start = sh.vaddr as usize;
            let end = start + sh.vsize as usize;
            println!("vaddr range            : {start:#018X}-{end:#018X}");
            start..end
        };
        let range_vaddr_aligned = {
            let start = range_vaddr.start & !align_mask;
            let end = (range_vaddr.end + align_mask) & !align_mask;
            start..end
        };
        let range_in_region = {
            let start = range_vaddr.start & align_mask;
            let end = start + sh.vsize as usize;
            start..end
        };

        let region_size = range_vaddr_aligned.end - range_vaddr_aligned.start;
        let mut region = ContiguousPhysicalMemoryPages::alloc_bytes(region_size)?;
        let region_slice = region.as_mut_slice();
        region_slice.fill(0);
        let region_start = region_slice.as_ptr() as u64;
        let region_end = region_slice.as_ptr() as u64 + region_slice.len() as u64;
        println!("Region allocated       : {region_start:#018X}-{region_end:#018X}",);
        println!(
            "Making {:#018X} - {:#018X} accessible to user mode",
            region_start, region_end
        );
        unsafe {
            with_current_page_table(|table| {
                table
                    .create_mapping(
                        region_start,
                        region_end,
                        region_start, // Identity Mapping
                        PageAttr::ReadWriteUser,
                    )
                    .expect("Failed to set mapping");
            });
        }

        let dst_start_ofs = range_vaddr.start & align_mask;
        let copy_size = range_file.end - range_file.start;
        let dst = &mut region_slice[dst_start_ofs..dst_start_ofs + copy_size];

        let src = elf.file.data();
        let src = &src[range_file];
        dst.copy_from_slice(src);

        let range_vaddr_relocated = {
            let start = region_start as usize + dst_start_ofs;
            let end = start + sh.vsize as usize;
            println!("vaddr range (relocated): {start:#018X}-{end:#018X}");
            start..end
        };

        Ok(LoadedSegment {
            region,
            range_in_region,
            range_vaddr,
            range_vaddr_relocated,
        })
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.region.as_mut_slice()[self.range_in_region.clone()]
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.region.as_slice()[self.range_in_region.clone()]
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct SectionHeader {
    name_ofs: u32,
    section_type: u32,
    flags: u32,
    vaddr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entry_size: u64,
}
impl fmt::Debug for SectionHeader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "type: {:#010X}", self.section_type)?;
        write!(f, ", offset: {:#018X}", self.offset)?;
        write!(f, ", vaddr: {:#018X}", self.vaddr)?;
        write!(f, ", align: {:#018X}", self.align)?;
        Ok(())
    }
}

pub struct LoadedElf<'a> {
    elf: &'a Elf<'a>,
    loaded_segments: Vec<LoadedSegment>,
}
impl<'a> LoadedElf<'a> {
    fn resolve_vaddr(&self, vaddr: usize) -> Result<usize> {
        for s in &self.loaded_segments {
            if s.range_vaddr.contains(&vaddr) {
                return Ok(vaddr
                    .wrapping_sub(s.range_vaddr.start)
                    .wrapping_add(s.range_vaddr_relocated.start));
            }
        }
        Err(Error::Failed("Not found"))
    }
    pub fn exec(self) -> Result<usize> {
        println!("LoadedElf::exec(file: {})", self.elf.file.name());
        let stack_size = 8 * 1024;
        let mut stack = ContiguousPhysicalMemoryPages::alloc_bytes(stack_size)?;
        let stack = stack.as_mut_slice();
        unsafe {
            core::ptr::write_bytes(stack.as_mut_ptr(), 0, stack.len());
        }
        let stack_start = stack.as_ptr() as u64;
        let stack_end = stack.as_ptr() as u64 + stack.len() as u64;
        println!("Stack allocated = {stack_start:#018X}-{stack_end:#018X}",);
        println!(
            "Making {:#018X} - {:#018X} accessible to user mode",
            stack_start, stack_end
        );
        unsafe {
            with_current_page_table(|table| {
                table
                    .create_mapping(
                        stack_start,
                        stack_end,
                        stack_start, // Identity Mapping
                        PageAttr::ReadWriteUser,
                    )
                    .expect("Failed to set mapping");
            });
        }
        let entry_point = self.resolve_vaddr(self.elf.entry_vaddr as usize)?;
        println!("entry_point = {:#018X}", entry_point);
        unsafe {
            let retcode: i64;
            asm!(
                // Set data segments to USER_DS
                "mov es, dx",
                "mov ds, dx",
                "mov fs, dx",
                "mov gs, dx",
                // Use iretq to switch to user mode
                "push rdx", // SS
                "push rax", // RSP
                "mov ax, 2",
                "push rax", // RFLAGS
                "push rcx", // CS
                "push rdi", // RIP
                // *(rip as *const InterruptContext) == {
                //   rip: u64,
                //   cs: u64,
                //   rflags: u64,
                //   rsp: u64,
                //   ss: u64,
                // }
                // far-jmp to app using ireq
                "iretq",
                // TODO(hikalium): check if it is a qemu's bug that the page fault becomes triple
                // fault when the code is mapped but in a supervisor mode.

                in("rax") stack_end, // stack grows toward 0, so empty stack pointer will be the end addr
                in("rdi") entry_point,
                in("rdx") crate::x86_64::USER_DS,
                in("rcx") crate::x86_64::USER64_CS,
                lateout("rax") retcode,
            );
            println!("returned from the code! retcode = {}", retcode);
        }
        Ok(0)
    }
}

pub struct Elf<'a> {
    file: &'a File,
    entry_vaddr: u64,
    segments: Vec<SegmentHeader>,
    sections: BTreeMap<String, SectionHeader>,
}
impl<'a> Elf<'a> {
    pub fn parse(file: &'a File) -> Result<Self> {
        println!("Elf::parse(file: {})", file.name());
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

        let index_of_string_table = read_le_u16(data, 62)?;

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
        let string_table = sections[index_of_string_table as usize];
        let string_table =
            &data[string_table.offset as usize..(string_table.offset + string_table.size) as usize];
        let sections: BTreeMap<String, SectionHeader> = sections
            .iter()
            .map(|s| {
                let section_name = &string_table[s.name_ofs as usize..];
                let section_name: Vec<u8> = section_name
                    .iter()
                    .cloned()
                    .take_while(|c| *c != 0)
                    .collect();
                let section_name = core::str::from_utf8(&section_name).unwrap_or("(invalid)");
                (section_name.to_string(), *s)
            })
            .collect();
        Ok(Self {
            file,
            entry_vaddr,
            segments,
            sections,
        })
    }
    pub fn load(&self) -> Result<LoadedElf> {
        println!("Elf::load(file: {})", self.file.name());
        println!("Segments:");
        for s in &self.segments {
            println!("{s:?}");
        }
        println!("Sections:");
        for (k, s) in &self.sections {
            println!("{k:16} {s:?}");
        }
        let sh_index_to_load: Vec<usize> = self
            .segments
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_i, s)| s.phdr_type == PHDR_TYPE_LOAD)
            .map(|(i, _s)| i)
            .collect();
        if sh_index_to_load.is_empty() {
            return Err(Error::Failed("LOAD segment not found"));
        }
        let mut loaded_segments: Vec<LoadedSegment> = sh_index_to_load
            .iter()
            .map(|i| LoadedSegment::new(self, *i))
            .collect::<Result<Vec<LoadedSegment>>>()?;
        if let Some(got) = self.sections.get(".got") {
            let got_segment_index = loaded_segments
                .iter()
                .enumerate()
                .find(|(_i, s)| s.range_vaddr.contains(&(got.vaddr as usize)))
                .ok_or(Error::Failed(
                    ".got section is found but there is no segment for it",
                ))?
                .0;
            let got_segment_start_vaddr = loaded_segments[got_segment_index].range_vaddr.start;
            let got_start_ofs_in_segment = got.vaddr as usize - got_segment_start_vaddr;
            let got_range_in_segment =
                got_start_ofs_in_segment..got_start_ofs_in_segment + got.size as usize;
            let mut got_values = {
                let got_data = &mut loaded_segments[got_segment_index].as_mut_slice()
                    [got_range_in_segment.clone()];
                let mut got_values = Vec::new();
                for i in 0..(got.size as usize) / 8 {
                    got_values.push(read_le_u64(got_data, i * 8)?);
                }
                got_values
            };
            got_values.iter_mut().enumerate().for_each(|(i, addr)| {
                print!(" GOT[{:#04X}]: {:#18X}", i, addr);
                for (i, s) in loaded_segments.iter().enumerate() {
                    if s.range_vaddr.contains(&(*addr as usize)) {
                        print!(" in section #{i} -> ");
                        *addr = (*addr)
                            .wrapping_sub(s.range_vaddr.start as u64)
                            .wrapping_add(s.range_vaddr_relocated.start as u64);
                        break;
                    }
                }
                println!("GOT[{:#04X}]: {:#18X}", i, addr);
            });
            {
                let got_data =
                    &mut loaded_segments[got_segment_index].as_mut_slice()[got_range_in_segment];
                for (i, addr) in got_values.iter().enumerate() {
                    write_le_u64(got_data, i * 8, *addr)?;
                }
            }
        }
        Ok(LoadedElf {
            elf: self,
            loaded_segments,
        })
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
