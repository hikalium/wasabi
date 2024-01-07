use crate::error::Result;
use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct CStrPtr16 {
    ptr: *const u16,
}
impl CStrPtr16 {
    pub fn from_ptr(p: *const u16) -> CStrPtr16 {
        CStrPtr16 { ptr: p }
    }
}

pub fn strlen_char16(strp: CStrPtr16) -> usize {
    let mut len: usize = 0;
    unsafe {
        loop {
            if *strp.ptr.add(len) == 0 {
                break;
            }
            len += 1;
        }
    }
    len
}

impl fmt::Display for CStrPtr16 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        unsafe {
            let mut index = 0;
            loop {
                let c = *self.ptr.offset(index);
                if c == 0 {
                    break;
                }
                let bytes = c.to_be_bytes();
                write!(f, "{}", bytes[1] as char)?;
                index += 1;
            }
        }
        fmt::Result::Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiGuid {
    pub data0: u32,
    pub data1: u16,
    pub data2: u16,
    pub data3: [u8; 8],
}

pub type EfiVoid = u8;

#[repr(C)]
#[derive(Debug)]
pub struct EfiTableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    reserved: u32,
}

pub type EfiHandle = u64;

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[must_use]
#[repr(u64)]
pub enum EfiStatus {
    SUCCESS = 0,
}
impl EfiStatus {
    pub fn into_result(self) -> Result<()> {
        if self == EfiStatus::SUCCESS {
            Ok(())
        } else {
            Err(self.into())
        }
    }
}
