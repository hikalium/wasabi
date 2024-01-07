extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::memory::AddressRange;
use core::fmt;
use core::mem::size_of;
use core::ops::Range;

pub const PHDR_TYPE_LOAD: u32 = 1;
pub const PHDR_TYPE_DYNAMIC: u32 = 2;

pub const DYNAMIC_TAG_RELA_ADDRESS: u64 = 7;
pub const DYNAMIC_TAG_RELA_TOTAL_SIZE: u64 = 8;
pub const DYNAMIC_TAG_RELA_ENTRY_SIZE: u64 = 9;

// [elf_1_2] Figure A-3. Relocation Types
// B: base address where the object file is loaded to
// A: addend field in the relocation entry
pub const R_386_RELATIVE: u64 = 8; // B + A

#[derive(Copy, Clone)]
#[allow(unused)]
#[repr(C)]
pub struct SegmentHeader {
    pub phdr_type: u32,
    pub entry_type: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub fsize: u64,
    pub vsize: u64,
    pub align: u64,
}
impl SegmentHeader {
    pub fn vaddr_range(&self) -> AddressRange {
        AddressRange::from_start_and_size(self.vaddr as usize, self.vsize as usize)
    }
    pub fn file_range(&self) -> Range<usize> {
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
    pub name_ofs: u32,
    pub section_type: u32,
    pub flags: u32,
    pub vaddr: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
    pub info: u32,
    pub align: u64,
    pub entry_size: u64,
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
    pub fn file_range(&self) -> Result<Range<usize>> {
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
    pub address: u64,
    pub info: u64,
    pub addend: u64,
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
    pub tag: u64,
    pub value: u64,
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
