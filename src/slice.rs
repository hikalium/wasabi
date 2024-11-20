extern crate alloc;

use crate::result::Result;
use core::mem::size_of;
use core::slice;

/// # Safety
/// Implementing this trait is safe only when the target type can be converted
/// mutually between a byte sequence of the same size, which means that no ownership
/// nor memory references are involved.
pub unsafe trait Sliceable: Sized + Copy + Clone {
    fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self as *const Self as *const u8, size_of::<Self>()) }
    }
    fn copy_from_slice(data: &[u8]) -> Result<Self> {
        if size_of::<Self>() > data.len() {
            Err("data is too short")
        } else {
            Ok(unsafe { *(data.as_ptr() as *const Self) })
        }
    }
}
