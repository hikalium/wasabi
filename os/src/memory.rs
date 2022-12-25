extern crate alloc;

use crate::allocator::ALLOCATOR;
use crate::error::Error;
use crate::error::Result;
use crate::util::PAGE_SIZE;
use alloc::boxed::Box;
use core::alloc::Layout;
use core::mem::ManuallyDrop;
use core::pin::Pin;
use core::slice;

pub fn alloc_pages(num_pages: usize) -> Result<Pin<Box<[u8]>>> {
    let size = PAGE_SIZE * num_pages;
    let scratchpad_buffers = ALLOCATOR.alloc_with_options(
        Layout::from_size_align(size, PAGE_SIZE)
            .map_err(|_| Error::Failed("could not allocate pages"))?,
    );
    let scratchpad_buffers = unsafe { slice::from_raw_parts(scratchpad_buffers as *mut u8, size) };
    Ok(Pin::new(Box::<[u8]>::from(scratchpad_buffers)))
}

pub struct Mmio<T: Sized> {
    inner: ManuallyDrop<Pin<Box<T>>>,
}
impl<T: Sized> Mmio<T> {
    /// # Safety
    /// Caller must ensure:
    /// - *ptr is valid
    /// - CPU Caches for the range pointed by ptr are disabled
    /// - No other party in this program have the ownership of *ptr
    pub unsafe fn from_raw(ptr: *mut T) -> Self {
        Self {
            inner: ManuallyDrop::new(Box::into_pin(Box::from_raw(ptr))),
        }
    }
    /// # Safety
    /// Same rules as Pin::get_unchecked_mut() applies.
    pub unsafe fn get_unchecked_mut(&mut self) -> &mut T {
        self.inner.as_mut().get_unchecked_mut()
    }
}
impl<T> AsRef<T> for Mmio<T> {
    fn as_ref(&self) -> &T {
        self.inner.as_ref().get_ref()
    }
}
