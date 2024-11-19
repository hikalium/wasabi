use crate::result::Result;
use core::pin::Pin;
use core::slice;

/// # Safety
/// Implementing this trait is safe only when the target type can be constructed from any byte
/// sequences that has the same size. If not, modification made via the byte slice produced by
/// as_mut_slice can be an undefined behavior since the bytes can not be interpreted as the
/// original type.
pub unsafe trait IntoPinnedMutableSlice: Sized + Copy + Clone {
    fn as_mut_slice(self: Pin<&mut Self>) -> Pin<&mut [u8]> {
        Pin::new(unsafe {
            slice::from_raw_parts_mut(
                self.get_unchecked_mut() as *mut Self as *mut u8,
                size_of::<Self>(),
            )
        })
    }
    fn as_mut_slice_sized(self: Pin<&mut Self>, size: usize) -> Result<Pin<&mut [u8]>> {
        if size > size_of::<Self>() {
            Err("Cannot take mut slice longer than the object")
        } else {
            Ok(Pin::new(unsafe {
                slice::from_raw_parts_mut(self.get_unchecked_mut() as *mut Self as *mut u8, size)
            }))
        }
    }
}
