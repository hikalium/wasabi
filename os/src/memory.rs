extern crate alloc;

use crate::allocator::ALLOCATOR;
use crate::error::Error;
use crate::error::Result;
use crate::println;
use crate::util::size_in_pages_from_bytes;
use crate::util::PAGE_SIZE;
use crate::x86_64::paging::with_current_page_table;
use crate::x86_64::paging::PageAttr;
use alloc::boxed::Box;
use core::alloc::Layout;
use core::fmt;
use core::mem::ManuallyDrop;
use core::ops::Range;
use core::pin::Pin;
use core::slice;

pub struct AddressRange {
    range: Range<usize>,
}
impl AddressRange {
    pub fn new(start: usize, end: usize) -> Self {
        let range = start..end;
        Self { range }
    }
    pub fn from_start_and_size(start: usize, size: usize) -> Self {
        Self::new(start, start + size)
    }
    pub fn start(&self) -> usize {
        self.range.start
    }
    pub fn end(&self) -> usize {
        self.range.end
    }
    pub fn size(&self) -> usize {
        self.range.end - self.range.start
    }
    pub fn into_range_in(&self, region: &AddressRange) -> Result<Range<usize>> {
        let start = region.offset_of(self.start())?;
        if self.size() == 0 || region.contains(self.end() - 1) {
            Ok(start..start + self.size())
        } else {
            Err(Error::Failed("end of this range is outside of the region"))
        }
    }
    pub fn contains(&self, addr: usize) -> bool {
        self.range.contains(&addr)
    }
    pub fn offset_of(&self, addr: usize) -> Result<usize> {
        if self.contains(addr) {
            Ok(addr - self.start())
        } else {
            Err(Error::Failed("addr is outside of this region"))
        }
    }
}
impl From<(usize, usize)> for AddressRange {
    fn from(e: (usize, usize)) -> Self {
        Self::new(e.0, e.1)
    }
}
impl From<(u64, u64)> for AddressRange {
    fn from(e: (u64, u64)) -> Self {
        Self::new(e.0 as usize, e.1 as usize)
    }
}
impl fmt::Debug for AddressRange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "addr({:#018X}-{:#018X})", self.start(), self.end())
    }
}

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
    pub fn fill_with_bytes(&mut self, value: u8) {
        unsafe {
            core::ptr::write_bytes(self.phys_addr, value, self.range().size());
        }
    }
    pub fn range(&self) -> AddressRange {
        AddressRange::from_start_and_size(self.phys_addr as usize, self.layout.size())
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: This is safe since byte-level access to the region is always aligned and no
        // value restrictions there. Shared access via the reference is avoided by the borrow
        // checker, by requiring &mut self.
        unsafe { slice::from_raw_parts_mut(self.phys_addr, self.layout.size()) }
    }
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: This is safe since byte-level access to the region is always aligned and no
        // value restrictions there. Shared write access via the reference is avoided by the borrow
        // checker, by requiring &self.
        unsafe { slice::from_raw_parts(self.phys_addr, self.layout.size()) }
    }
    pub fn set_page_attr(&mut self, attr: PageAttr) -> Result<()> {
        let range = self.range();
        println!("Setting page attr for {:?} to {:?}", range, attr);
        unsafe {
            with_current_page_table(|table| {
                table
                    .create_mapping(
                        range.start() as u64,
                        range.end() as u64,
                        range.start() as u64, // Identity Mapping
                        attr,
                    )
                    .expect("Failed to set mapping");
            });
        }
        Ok(())
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
    let scratchpad_buffers = unsafe { slice::from_raw_parts(scratchpad_buffers, size) };
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
