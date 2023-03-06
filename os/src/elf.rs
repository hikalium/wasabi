extern crate alloc;

use crate::boot_info::File;
use crate::error::Error;
use crate::error::Result;
use crate::print;
use crate::println;
use crate::util::read_le_u16;
use crate::util::read_le_u32;
use crate::util::read_le_u64;
use crate::util::size_in_pages_from_bytes;
use crate::x86_64::paging::with_current_page_table;
use crate::x86_64::paging::PageAttr;
use alloc::borrow::BorrowMut;
use alloc::boxed::Box;
use alloc::slice;
use alloc::vec::Vec;
use core::arch::asm;
use core::fmt;
use core::mem::size_of;
use core::ops::Range;

#[repr(align(4096))]
#[allow(dead_code)]
struct Page4K {
    bytes: [u8; 4096],
}
impl Page4K {
    fn alloc_contiguous(byte_size: usize) -> Box<[Page4K]> {
        let uninit_slice = Box::<[Page4K]>::new_uninit_slice(size_in_pages_from_bytes(byte_size));
        unsafe {
            // This is safe since any bytes for Page4K is valid
            uninit_slice.assume_init()
        }
    }
    fn into_u8_slice_mut(src: &mut [Self]) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(src.as_mut_ptr() as *mut u8, src.len() * size_of::<Page4K>())
        }
    }
}

pub struct LoadInfo {
    segments: Vec<Segment>,
    entry_vaddr: u64,
}

const PHDR_TYPE_LOAD: u32 = 1;

#[derive(Copy, Clone)]
pub struct Segment {
    phdr_type: u32,
    entry_type: u32,
    offset: u64,
    vaddr: u64,
    fsize: u64,
    vsize: u64,
    align: u64,
}
impl Segment {
    fn src_range(&self) -> Range<usize> {
        // range of offset in the file
        self.offset as usize..(self.offset + self.fsize) as usize
    }
    fn dst_range(&self) -> Range<usize> {
        // range of (program's) virtual address
        self.vaddr as usize..(self.vaddr + self.vsize) as usize
    }
}

#[derive(Copy, Clone)]
#[allow(unused)]
#[repr(C)]
pub struct Section {
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

pub struct Elf<'a> {
    file: &'a File,
}
impl<'a> Elf<'a> {
    pub fn new(file: &'a File) -> Self {
        Self { file }
    }
    pub fn parse(&self) -> Result<LoadInfo> {
        let data = self.file.data();
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
        println!("This ELF seems to be executable!");

        let entry_vaddr = read_le_u64(data, 24)?;

        let phdr_start = read_le_u64(data, 32)?;
        let phdr_entry_size = read_le_u16(data, 54)?;
        let num_of_phdr_entry = read_le_u16(data, 56)?;

        let shdr_start = read_le_u64(data, 40)?;
        let shdr_entry_size = read_le_u16(data, 58)?;
        let num_of_shdr_entry = read_le_u16(data, 60)?;

        let index_of_string_table = read_le_u16(data, 62)?;

        println!(
            "phdr_start: {}, phdr_entry_size: {}, num_of_phdr_entry: {}",
            phdr_start, phdr_entry_size, num_of_phdr_entry
        );

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
            segments.push(Segment {
                phdr_type,
                entry_type,
                offset,
                vaddr,
                fsize,
                vsize,
                align,
            });
        }
        println!("Segments:");
        for s in &segments {
            print!("type: {:#010X}", s.phdr_type);
            print!(", attr(rwx): ({:03b})", s.entry_type); // RWX
            print!(", offset: {:#018X}", s.offset);
            print!(", vaddr: {:#018X}", s.vaddr);
            print!(", fsize: {:#018X}", s.fsize);
            print!(", vsize: {:#018X}", s.vsize);
            print!(", align: {:#018X}", s.align);
            println!("");
        }
        let segments: Vec<Segment> = segments
            .iter()
            .cloned()
            .filter(|s| s.phdr_type == PHDR_TYPE_LOAD)
            .collect();
        if segments.is_empty() {
            return Err(Error::Failed("LOAD segment not found"));
        }

        let shdr_indexes = 0..num_of_shdr_entry;
        let mut sections = Vec::new();
        for i in shdr_indexes {
            let ofs = (shdr_start + i as u64 * shdr_entry_size as u64) as usize;
            let shdr_entry = &data[ofs..(ofs + shdr_entry_size as usize)];
            if shdr_entry.len() < size_of::<Section>() {
                return Err(Error::Failed("Section size is invalid"));
            }
            let section = shdr_entry.as_ptr() as *const Section;
            // SAFETY: This is safe since the check above ensures the bounds
            let section = unsafe { *section };
            sections.push(section);
        }
        let string_table = sections[index_of_string_table as usize];
        let string_table =
            &data[string_table.offset as usize..(string_table.offset + string_table.size) as usize];
        println!("Sections:");
        for s in &sections {
            let section_name = &string_table[s.name_ofs as usize..];
            let section_name: Vec<u8> = section_name
                .iter()
                .cloned()
                .take_while(|c| *c != 0)
                .collect();
            let section_name = core::str::from_utf8(&section_name).unwrap_or("(invalid)");
            print!("{:16}", section_name);
            print!("type: {:#010X}", s.section_type);
            print!(", offset: {:#018X}", s.offset);
            print!(", vaddr: {:#018X}", s.vaddr);
            print!(", align: {:#018X}", s.align);
            println!("");
            if section_name == ".got" {
                let got_data = &data[s.offset as usize..(s.offset + s.size) as usize];
                for i in 0..(s.size as usize) / 8 {
                    println!(" GOT[{:#04X}]: {:#18X}", i, read_le_u64(got_data, i * 8)?);
                }
            }
        }

        Ok(LoadInfo {
            segments,
            entry_vaddr,
        })
    }
    pub fn exec(&self) -> Result<()> {
        let load_info = self.parse()?;
        let mut load_region_start = u64::MAX;
        let mut load_region_end = 0;
        for s in &load_info.segments {
            let start = s.vaddr;
            let end = s.vaddr + s.vsize;
            load_region_start = core::cmp::min(load_region_start, start);
            load_region_end = core::cmp::max(load_region_end, end);
        }
        load_region_start &= !0xFFF;
        load_region_end = (load_region_end + 0xFFF) & !0xFFF;
        println!("load_region_start = {load_region_start:#018X}");
        println!("load_region_end = {load_region_end:#018X}");
        let load_region_size = load_region_end - load_region_start;
        let mut load_region = Page4K::alloc_contiguous(load_region_size as usize);
        let load_region = Page4K::into_u8_slice_mut(load_region.borrow_mut());
        unsafe {
            core::ptr::write_bytes(load_region.as_mut_ptr(), 0, load_region.len());
        }
        println!("load_region @ {:#p}", load_region.as_ptr());
        let app_mem_start = load_region.as_ptr() as u64;
        let app_mem_end = app_mem_start + load_region.len() as u64;
        println!(
            "Making {:#018X} - {:#018X} accessible to user mode",
            app_mem_start, app_mem_end
        );
        unsafe {
            with_current_page_table(|table| {
                table
                    .create_mapping(
                        app_mem_start,
                        app_mem_end,
                        app_mem_start,
                        PageAttr::ReadWriteUser,
                    )
                    .expect("Failed to set mapping");
            });
        }

        let data = self.file.data();
        for s in &load_info.segments {
            let src_range = s.src_range();
            let src_range_size = src_range.end - src_range.start;
            println!(
                "src_range: {:#018X} - {:#018X}",
                src_range.start, src_range.end
            );
            let src = &data[src_range];

            let dst_range = s.dst_range();
            let dst_range_size = dst_range.end - dst_range.start;
            let copy_size = src_range_size;
            let fill_size = dst_range_size - src_range_size;
            let dst_range_start = dst_range.start - load_region_start as usize;
            let dst_range_copy = dst_range_start..dst_range_start + copy_size;
            let dst_range_fill = dst_range_copy.end..dst_range_copy.end + fill_size;
            println!(
                "dst_range(copy): {:#018X} - {:#018X}",
                dst_range.start, dst_range.end
            );
            load_region[dst_range_copy].copy_from_slice(src);
            load_region[dst_range_fill].fill(0);
        }
        println!("run the code!");
        let entry_point = unsafe {
            load_region
                .as_ptr()
                .add((load_info.entry_vaddr - load_region_start) as usize)
        };
        println!("entry_point = {:#p}", entry_point);
        unsafe {
            let retcode: i64;
            asm!(
                // Use iretq to switch to user mode
                "mov rbp, rsp",
                "push rdx", // SS
                "push rbp", // RSP
                "mov ax, 2",
                "push rax", // RFLAGS
                "push rcx", // CS
                "lea rax, [rip+1f]",
                "push rdi", // RIP
                // *(rip as *const InterruptContext) == {
                //   rip: u64,
                //   cs: u64,
                //   rflags: u64,
                //   rsp: u64,
                //   ss: u64,
                // }
                "iretq",

                "1:",
                "jmp rdi",
                // Set data segments to USER_DS
                "mov es, dx",
                "mov ds, dx",
                "mov fs, dx",
                "mov gs, dx",
                // Now, the CPU is in the user mode. Call the apps entry pointer
                // (rax is set by rust, via the asm macro params)
                "call rax",
                // Call exit() when it is returned
                "mov rdi, rax", // retcode = rax
                "mov eax, 0", // op = exit (0)
                "syscall",
                "ud2",
                in("rdi") entry_point,
                in("rdx") crate::x86_64::USER_DS,
                in("rcx") crate::x86_64::USER64_CS,
                lateout("rax") retcode,
            );
            println!("returned from the code! retcode = {}", retcode);
        }

        Ok(())
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
