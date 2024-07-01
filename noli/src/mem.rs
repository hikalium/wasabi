extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use alloc::boxed::Box;
use core::mem::size_of;
use core::mem::MaybeUninit;
use core::slice;

/// # Safety
/// Implementing this trait is safe only when the target type can be converted
/// mutually between a byte sequence of the same size, which means that no ownership
/// nor memory references are involved.
pub unsafe trait Sliceable: Sized + Copy + Clone {
    fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self as *const Self as *const u8, size_of::<Self>()) }
    }
    fn from_slice(slice: &[u8]) -> Result<&Self> {
        if slice.len() >= size_of::<Self>() {
            // SAFETY: the memory region referenced via the returned reference resides in the slice
            // Also, the alignment restriction should be satisfied if Self is marked as packed <<
            // really?
            Ok(unsafe { &*(slice.as_ptr() as *const Self) })
        } else {
            Err(Error::Failed("Sliceable::from_slice: Too small"))
        }
    }
    fn from_slice_mut(slice: &mut [u8]) -> Result<&mut Self> {
        if slice.len() >= size_of::<Self>() {
            // SAFETY: the memory region referenced via the returned reference resides in the slice
            // Also, the alignment restriction should be satisfied if Self is marked as packed <<
            // really?
            Ok(unsafe { &mut *(slice.as_ptr() as *mut Self) })
        } else {
            Err(Error::Failed("Sliceable::from_slice_mut: Too small"))
        }
    }
    fn copy_into_slice(&self) -> Box<[u8]> {
        let mut values = Box::<[u8]>::new_uninit_slice(size_of::<Self>());
        unsafe {
            values.copy_from_slice(slice::from_raw_parts(
                self as *const Self as *const MaybeUninit<u8>,
                size_of::<Self>(),
            ));
            values.assume_init()
        }
    }
    fn copy_from_slice(data: &[u8]) -> Result<Self> {
        if size_of::<Self>() > data.len() {
            Err(Error::Failed("data is too short"))
        } else {
            Ok(unsafe { *(data.as_ptr() as *const Self) })
        }
    }
}
