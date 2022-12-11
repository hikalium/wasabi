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

pub struct SegmentToLoad {
    entry_type: u32,
    entry_vaddr: u64,
    offset: u64,
    vaddr: u64,
    fsize: u64,
    vsize: u64,
    align: u64,
}
impl SegmentToLoad {
    fn src_range(&self) -> Range<usize> {
        self.offset as usize..(self.offset + self.fsize) as usize
    }
    fn dst_range(&self) -> Range<usize> {
        0..self.fsize as usize
    }
}

pub struct Elf<'a> {
    file: &'a File,
}
impl<'a> Elf<'a> {
    pub fn new(file: &'a File) -> Self {
        Self { file }
    }
    pub fn parse(&self) -> Result<Vec<SegmentToLoad>> {
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
                entry_vaddr,
                offset,
                vaddr,
                fsize,
                vsize,
                align,
            });
        }
        if segment_info.len() == 0 {
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

        Ok(segment_info)
    }
    pub fn exec(&self) -> Result<()> {
        let segment_to_load = self.parse()?;

        for s in &segment_to_load {
            if s.entry_type & 0b001 == 0 {
                // TODO(hikalium): Impl data section loading
                continue;
            }
            let mut code_pages = Page4K::alloc_contiguous(s.vaddr as usize);
            let code_dst = Page4K::into_u8_slice_mut(code_pages.borrow_mut());
            let data = self.file.data();
            let code_src = &data[s.src_range()];
            code_dst[s.dst_range()].copy_from_slice(code_src);
            println!("run the code!");
            let rel_code_entry_ofs = s.entry_vaddr - s.vaddr;
            let retcode: i64;
            unsafe {
                asm!("call rax",
                    // Call exit() when it is returned
                    "mov ecx, eax",
                    "mov eax, 0 // exit",
                    "syscall",
                    "ud2",
                    in("rax") code_dst.as_ptr().add(rel_code_entry_ofs as usize),
                    lateout("rax") retcode,
                );
            }
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
