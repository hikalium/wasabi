extern crate alloc;

use crate::allocator::ALLOCATOR;
use crate::error::Error;
use crate::error::Result;
use crate::util::size_in_pages_from_bytes;
use crate::util::PAGE_SIZE;
use alloc::boxed::Box;
use core::alloc::Layout;
use core::mem::ManuallyDrop;
use core::pin::Pin;
use core::slice;

/// Represents a physically-contiguous region of
/// 4KiB memory pages.
/// The allocated region will be uninitialized
pub struct ContiguousPhysicalMemoryPages {
    layout: Layout,
    phys_addr: *mut u8,
}
impl ContiguousPhysicalMemoryPages {
    pub fn alloc_pages(num_pages: usize) -> Result<Self> {
        let layout = Layout::from_size_align(PAGE_SIZE * num_pages, PAGE_SIZE)
            .or(Err(Error::Failed("Invalid layout")))?;
        let phys_addr = ALLOCATOR.alloc_with_options(layout);
        Ok(Self { layout, phys_addr })
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: This is safe since byte-level access to the region is always aligned and no
        // value restrictions there. Shared access via the reference is avoided by the borrow
        // checker, by requiring &mut self.
        unsafe { slice::from_raw_parts_mut(self.phys_addr as *mut u8, self.layout.size()) }
    }
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: This is safe since byte-level access to the region is always aligned and no
        // value restrictions there. Shared write access via the reference is avoided by the borrow
        // checker, by requiring &self.
        unsafe { slice::from_raw_parts(self.phys_addr as *mut u8, self.layout.size()) }
    }
    /// Allocates a physically-contiguous region of 4KiB memory pages that has enough space to
    /// `num_bytes` bytes.
    pub fn alloc_bytes(num_bytes: usize) -> Result<Self> {
        Self::alloc_pages(size_in_pages_from_bytes(num_bytes))
    }
}

// TODO(hikalium): replace this with ContiguousPhysicalMemoryPages
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
