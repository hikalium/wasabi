extern crate alloc;

use crate::boot_info::File;
use crate::error::Error;
use crate::error::Result;
use crate::memory::AddressRange;
use crate::memory::ContiguousPhysicalMemoryPages;
use crate::println;
use crate::util::read_le_u16;
use crate::util::read_le_u32;
use crate::util::read_le_u64;
use crate::x86_64::paging::PageAttr;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::arch::asm;
use core::cmp::max;
use core::cmp::min;
use core::fmt;
use core::mem::size_of;
use core::ops::Range;

const PHDR_TYPE_LOAD: u32 = 1;
const PHDR_TYPE_DYNAMIC: u32 = 2;

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
impl SegmentHeader {
    fn vaddr_range(&self) -> AddressRange {
        AddressRange::from_start_and_size(self.vaddr as usize, self.vsize as usize)
    }
    fn file_range(&self) -> Range<usize> {
        self.offset as usize..(self.offset + self.fsize) as usize
    }
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
impl SectionHeader {
    fn vaddr_range(&self) -> Result<AddressRange> {
        if self.vaddr != 0 && self.size != 0 {
            Ok(AddressRange::from_start_and_size(
                self.vaddr as usize,
                self.size as usize,
            ))
        } else {
            Err(Error::Failed(
                "This section does not have a valid vaddr_range",
            ))
        }
    }
    fn file_range(&self) -> Result<Range<usize>> {
        if self.offset != 0 && self.size != 0 {
            Ok(self.offset as usize..(self.offset + self.size) as usize)
        } else {
            Err(Error::Failed(
                "This section does not have a valid file_range",
            ))
        }
    }
}
impl fmt::Debug for SectionHeader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "type: {:#010X}", self.section_type)?;
        write!(f, ", align: {:#018X}", self.align)?;
        if let Ok(range) = self.file_range() {
            write!(f, ", file_range: {:?}", range)?;
        }
        if let Ok(range) = self.vaddr_range() {
            write!(f, ", vaddr_range: {:?}", range)?;
        }
        if self.entry_size != 0 {
            write!(f, ", entry_size: {:#018X}", self.entry_size)?;
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct SymbolTableEntry {
    name_ofs: u32,
    info: u8,
    _reserved: u8,
    section_index: u16,
    address: u64,
    size: u64,
}
impl fmt::Debug for SymbolTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "sym type {:X} bind {:X} addr {:#018X} size {:#018X}",
            self.info & 0x0F,
            (self.info >> 4) & 0x0F,
            self.address,
            self.size,
        )?;
        match self.section_index {
            0xFF00..=0xFF3F => {}
            0xFFF1 => write!(f, " SHN_ABS")?,
            0xFFF2 => write!(f, " SHN_COMMON")?,
            _ => write!(f, " @ section[{}]", self.section_index)?,
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct RelocationEntry {
    address: u64,
    info: u64,
    addend: u64,
}
impl fmt::Debug for RelocationEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "rel info {:#018X} addr {:#018X} addend {:#018X}",
            self.info, self.address, self.addend,
        )
    }
}
impl core::convert::TryFrom<&[u8]> for RelocationEntry {
    type Error = Error;
    fn try_from(data: &[u8]) -> Result<Self> {
        if size_of::<Self>() <= data.len() {
            // SAFETY: Following dereference is safe since the check above ensures its bounds
            Ok(unsafe { *(data.as_ptr() as *const RelocationEntry) })
        } else {
            Err(Error::Failed("data is too short for SymbolTableEntry"))
        }
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct DynamicEntry {
    tag: u64,
    value: u64,
}
impl fmt::Debug for DynamicEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "dyn tag {:#018X} value {:#018X}", self.tag, self.value,)
    }
}
impl core::convert::TryFrom<&[u8]> for DynamicEntry {
    type Error = Error;
    fn try_from(data: &[u8]) -> Result<Self> {
        if size_of::<Self>() <= data.len() {
            // SAFETY: Following dereference is safe since the check above ensures its bounds
            Ok(unsafe { *(data.as_ptr() as *const DynamicEntry) })
        } else {
            Err(Error::Failed("data is too short for DynamicEntry"))
        }
    }
}

pub struct LoadedElf<'a> {
    elf: &'a Elf<'a>,
    region: ContiguousPhysicalMemoryPages,
    app_vaddr_range: AddressRange,
    loaded_segments: Vec<&'a SegmentHeader>,
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
    pub fn exec(self) -> Result<usize> {
        println!("LoadedElf::exec(file: {})", self.elf.file.name());
        let stack_size = 8 * 1024;
        let mut stack = ContiguousPhysicalMemoryPages::alloc_bytes(stack_size)?;
        let stack_range = stack.range();
        println!("Stack allocated = {stack_range:?}",);
        stack.fill_with_bytes(0);
        stack.set_page_attr(PageAttr::ReadWriteUser)?;
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

                in("rax") stack_range.end(), // stack grows toward 0, so empty stack pointer will be the end addr
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
            sections,
            string_table,
        })
    }
    /// Load a segment to the region
    fn load_segment(
        &self,
        region: &mut ContiguousPhysicalMemoryPages,
        app_vaddr_range: &AddressRange,
        sh: &SegmentHeader,
    ) -> Result<()> {
        let segment_vaddr_range = sh.vaddr_range();
        let segment_file_range = sh.file_range();
        println!("Loading Segment: {segment_vaddr_range:?}...");

        let dst = region.as_mut_slice();
        let src = self.file.data();

        let dst = &mut dst[sh.vaddr_range().into_range_in(app_vaddr_range)?];
        let src = &src[segment_file_range];
        dst.copy_from_slice(src);

        Ok(())
    }
    pub fn load(&self) -> Result<LoadedElf> {
        println!("Elf::load(file: {})", self.file.name());
        println!("Segments:");
        for s in &self.segments {
            println!("{s:?}");
        }
        println!("Sections:");
        for (i, s) in &self.sections {
            println!("{i:16} {s:?}");
        }
        let segments_to_be_loaded: Vec<&SegmentHeader> = self
            .segments
            .iter()
            .filter(|s| s.phdr_type == PHDR_TYPE_LOAD)
            .collect();
        if segments_to_be_loaded.is_empty() {
            return Err(Error::Failed("LOAD segment not found"));
        }
        for s in &segments_to_be_loaded {
            println!("{s:?}");
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

        let dynamic_segment: Option<&SegmentHeader> = self
            .segments
            .iter()
            .find(|s| s.phdr_type == PHDR_TYPE_DYNAMIC);
        if let Some(dynamic_segment) = dynamic_segment {
            println!("DYNAMIC segment found")
        }

        /*
        if let Some(symbol_table) = self.sections.get(".symtab") {
            let range = symbol_table.file_range()?;
            let data = &self.file.data()[range];
            for i in 0..(symbol_table.size as usize) / symbol_table.entry_size as usize {
                let data = &data[i * symbol_table.entry_size as usize
                    ..(i + 1) * symbol_table.entry_size as usize];
                if data.len() < size_of::<SymbolTableEntry>() {
                    println!("Invalid symbol table size");
                    break;
                }
                // SAFETY: Following dereference is safe since the check above ensures its bounds
                let s = unsafe { *(data.as_ptr() as *const SymbolTableEntry) };
                println!(
                    "symbol[{:3}]: {:?} {}",
                    i,
                    s,
                    self.read_string(s.name_ofs as usize)
                );
            }
        }
        if let Some(relocation_table) = self.sections.get(".rela.dyn") {
            let vaddr_range = relocation_table
                .vaddr_range()?
                .into_range_in(&app_vaddr_range)?;
            for i in 0..(relocation_table.size as usize) / relocation_table.entry_size as usize {
                let ofs = i * relocation_table.entry_size as usize;
                let rel = if let Ok(e) =
                    RelocationEntry::try_from(&region.as_mut_slice()[vaddr_range.clone()][ofs..])
                {
                    println!("symbol[{:3}]: {:?}", i, e,);
                    Some((e.address, e.addend))
                } else {
                    None
                };
                if let Some(rel) = rel {
                    let data = &mut region.as_mut_slice()[vaddr_range.clone()];
                    let old = read_le_u64(data, rel.0 as usize)?;
                    println!("old: {old:#018X}");
                }
            }
        }
        */
        /*
        if let Some(got) = self.sections.get(".got") {
            let got_range = got.vaddr_range()?.into_range_in(&app_vaddr_range)?;
            let mut got_values = {
                let got_data = &mut region.as_mut_slice()[got_range.clone()];
                let mut got_values = Vec::new();
                for i in 0..(got.size as usize) / 8 {
                    got_values.push(read_le_u64(got_data, i * 8)?);
                }
                got_values
            };
            got_values.iter_mut().enumerate().for_each(|(i, addr)| {
                print!(" GOT[{:#04X}]: {:#18X}", i, addr);
                for (i, s) in loaded_segments.iter().enumerate() {
                    if s.vaddr_range().contains(*addr as usize) {
                        print!(" in section #{i} -> ");
                        *addr = (*addr)
                            .wrapping_sub(app_vaddr_range.start() as u64)
                            .wrapping_add(region.range().start() as u64);
                        break;
                    }
                }
                println!("GOT[{:#04X}]: {:#18X}", i, addr);
            });
            let got_data = &mut region.as_mut_slice()[got_range];
            for (i, &addr) in got_values.iter().enumerate() {
                println!("GOT[{:#04X}]: {:#18X}", i, addr);
                write_le_u64(got_data, i * 8, addr)?;
            }
        }*/
        Ok(LoadedElf {
            elf: self,
            region,
            app_vaddr_range,
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
