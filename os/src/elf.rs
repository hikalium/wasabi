extern crate alloc;

use crate::boot_info::File;
use crate::error::Error;
use crate::error::Result;
use crate::println;
use crate::util::read_le_u16;
use crate::util::read_le_u32;
use crate::util::read_le_u64;
use crate::util::size_in_pages_from_bytes;
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
    segments: Vec<SegmentToLoad>,
    entry_vaddr: u64,
}

pub struct SegmentToLoad {
    entry_type: u32,
    offset: u64,
    vaddr: u64,
    fsize: u64,
    vsize: u64,
    align: u64,
}
impl SegmentToLoad {
    fn src_range(&self) -> Range<usize> {
        // range of offset in the file
        self.offset as usize..(self.offset + self.fsize) as usize
    }
    fn dst_range(&self) -> Range<usize> {
        // range of (program's) virtual address
        self.vaddr as usize..(self.vaddr + self.vsize) as usize
    }
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
        if data[4] != 2 {
            return Err(Error::Failed("Not a 64-bit ELF"));
        }
        if data[5] != 1 {
            return Err(Error::Failed("Not a litte endian ELF"));
        }
        if data[7] != 0 {
            return Err(Error::Failed("ABI is not SystemV"));
        }
        if read_le_u16(data, 16)? != 2 {
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

        println!(
            "phdr_start: {}, phdr_entry_size: {}, num_of_phdr_entry: {}",
            phdr_start, phdr_entry_size, num_of_phdr_entry
        );

        // Find LOAD segmnet
        let phdr_indexes = 0..num_of_phdr_entry;
        let mut segment_info = Vec::new();
        for i in phdr_indexes {
            let ofs = (phdr_start + i as u64 * phdr_entry_size as u64) as usize;
            let phdr_entry = &data[ofs..(ofs + phdr_entry_size as usize)];
            let phdr_type = read_le_u32(phdr_entry, 0)?;
            if phdr_type != 1 {
                continue;
            }
            // type == LOAD (1)
            let entry_type = read_le_u32(phdr_entry, 4)?;
            let offset = read_le_u64(phdr_entry, 8)?;
            let vaddr = read_le_u64(phdr_entry, 16)?;
            let fsize = read_le_u64(phdr_entry, 32)?;
            let vsize = read_le_u64(phdr_entry, 40)?;
            let align = read_le_u64(phdr_entry, 48)?;
            segment_info.push(SegmentToLoad {
                entry_type,
                offset,
                vaddr,
                fsize,
                vsize,
                align,
            });
        }
        if segment_info.is_empty() {
            return Err(Error::Failed("LOAD segment not found"));
        }

        for s in &segment_info {
            println!("type  : (rwx) = ({:03b})", s.entry_type); // RWX
            println!("offset: {:#018X}", s.offset);
            println!("vaddr : {:#018X}", s.vaddr);
            println!("fsize : {:#018X}", s.fsize);
            println!("vsize : {:#018X}", s.vsize);
            println!("align : {:#018X}", s.align);
            println!("");
        }

        Ok(LoadInfo {
            segments: segment_info,
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

        let data = self.file.data();
        for s in &load_info.segments {
            let code_src = &data[s.src_range()];
            let dst_range = s.dst_range();
            let dst_range = (dst_range.start - load_region_start as usize)
                ..(dst_range.end - load_region_start as usize);
            println!(
                "dst_range: {:#018X} - {:#018X}",
                dst_range.start, dst_range.end
            );
            load_region[dst_range].copy_from_slice(code_src);
        }
        println!("run the code!");
        let entry_point = unsafe {
            load_region
                .as_ptr()
                .add((load_info.entry_vaddr - load_region_start) as usize)
        };
        println!("entry_point = {:#p}", entry_point);
        /*
            unsafe {
                asm!(".byte 0xeb, 0xfe" /* infinite loop */,);
            }
        */
        let retcode: i64;
        unsafe {
            #![warn(named_asm_labels)]
            asm!(
                "call rax",
                // Call exit() when it is returned
                /*
                ".global app_entry_point",
                "app_entry_point:",
                "3:",
                "jmp [rip+app_entry_point]",
                "lea rcx,[rip+1f]",
                "2:",
                "jmp [rip+2b]",
                "sysretq",
                "1:",
                "jmp [rip+1b]",
                */
                "mov rdi, rax", // retcode = rax
                "mov eax, 0", // op = exit (0)
                "syscall",
                "ud2",
                in("rax") entry_point,
                lateout("rax") retcode,
            );
        }
        println!("returned from the code! retcode = {}", retcode);

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
