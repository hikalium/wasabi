extern crate alloc;

use crate::efi::EfiMemoryDescriptor;
use crate::efi::EfiMemoryType;
use crate::memory_map_holder::MemoryMapHolder;
use crate::println;
use crate::util::round_up_to_nearest_pow2;
use alloc::alloc::GlobalAlloc;
use alloc::alloc::Layout;
use alloc::boxed::Box;
use core::borrow::BorrowMut;
use core::cell::RefCell;
use core::cmp::max;
use core::fmt;
use core::mem::size_of;
use core::ops::DerefMut;
use core::ptr::null_mut;

#[cfg(test)]
use alloc::vec;

/// Each char represents 32-byte chunks.
/// Vertical bar `|` represents the chunk that has a Header
/// before: |-- prev -------|---- self ---------------
/// align:  |--------|-------|-------|-------|-------|
/// after:  |---------------||-------|----------------

struct Header {
    next_header: Option<Box<Header>>,
    size: usize,
    is_allocated: bool,
    _reserved: usize,
}
const HEADER_SIZE: usize = size_of::<Header>();
#[allow(clippy::assertions_on_constants)]
const _: () = assert!(HEADER_SIZE == 32);
// Size of Header should be power of 2
const _: () = assert!(HEADER_SIZE.count_ones() == 1);
pub const LAYOUT_PAGE_4K: Layout = Layout::from_size_align(4096, 4096).ok().unwrap();
impl Header {
    fn can_provide(&self, size: usize, align: usize) -> bool {
        self.size >= size + HEADER_SIZE * 3 + align
    }
    fn is_allocated(&self) -> bool {
        self.is_allocated
    }
    fn end_addr(&self) -> usize {
        self as *const Header as usize + self.size
    }
    unsafe fn new_from_addr(addr: usize) -> Box<Header> {
        let header = addr as *mut Header;
        header.write(Header {
            next_header: None,
            size: 0,
            is_allocated: false,
            _reserved: 0,
        });
        Box::from_raw(addr as *mut Header)
    }
    unsafe fn from_allocated_region(addr: *mut u8) -> Box<Header> {
        let header = addr.sub(HEADER_SIZE) as *mut Header;
        Box::from_raw(header)
    }
    //
    // Note: std::alloc::Layout doc says:
    // > All layouts have an associated size and a power-of-two alignment.
    fn provide(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        let size = max(round_up_to_nearest_pow2(size).ok()?, HEADER_SIZE);
        let align = max(align, HEADER_SIZE);
        if self.is_allocated() || !self.can_provide(size, align) {
            None
        } else {
            // |-----|----------------- self ---------|----------
            // |-----|----------------------          |----------
            //                                        ^ self.end_addr()
            //                              |-------|-
            //                               ^ allocated_addr
            //                              ^ header_for_allocated
            //                                      ^ header_for_padding
            //                                      ^ header_for_allocated.end_addr()
            // self has enough space to allocate the requested object.

            // Make a Header for the allocated object
            let mut size_used = 0;
            let allocated_addr = (self.end_addr() - size) & !(align - 1);
            let mut header_for_allocated =
                unsafe { Self::new_from_addr(allocated_addr - HEADER_SIZE) };
            header_for_allocated.is_allocated = true;
            header_for_allocated.size = size + HEADER_SIZE;
            size_used += header_for_allocated.size;
            header_for_allocated.next_header = self.next_header.take();
            if header_for_allocated.end_addr() != self.end_addr() {
                // Make a Header for padding
                let mut header_for_padding =
                    unsafe { Self::new_from_addr(header_for_allocated.end_addr()) };
                header_for_padding.is_allocated = false;
                header_for_padding.size = self.end_addr() - header_for_allocated.end_addr();
                size_used += header_for_padding.size;
                header_for_padding.next_header = header_for_allocated.next_header.take();
                header_for_allocated.next_header = Some(header_for_padding);
            }
            // Shrink self
            assert!(self.size >= size_used + HEADER_SIZE);
            self.size -= size_used;
            self.next_header = Some(header_for_allocated);
            Some(allocated_addr as *mut u8)
        }
    }
}
impl Drop for Header {
    fn drop(&mut self) {
        panic!("Header should not be dropped!");
    }
}
impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Header @ {:#018X} {{ size: {:#018X}, is_allocated: {} }}",
            self as *const Header as usize,
            self.size,
            self.is_allocated()
        )
    }
}

pub struct FirstFitAllocator {
    first_header: RefCell<Option<Box<Header>>>,
}

#[global_allocator]
pub static ALLOCATOR: FirstFitAllocator = FirstFitAllocator {
    first_header: RefCell::new(None),
};

unsafe impl Sync for FirstFitAllocator {}

unsafe impl GlobalAlloc for FirstFitAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let addr = self.alloc_with_options(layout);
        //println!("ALLOCATOR! {:#p} {} alloc", addr, layout.size());
        addr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let mut region = Header::from_allocated_region(ptr);
        region.is_allocated = false;
        Box::leak(region);
        //println!("ALLOCATOR! {:#p} {} dealloc", ptr, layout.size());
        // region is leaked here to avoid dropping the free info on the memory.
    }
}

impl FirstFitAllocator {
    pub fn alloc_with_options(&self, layout: Layout) -> *mut u8 {
        let mut header = self.first_header.borrow_mut();
        let mut header = header.deref_mut();
        loop {
            match header {
                Some(e) => match e.provide(layout.size(), layout.align()) {
                    Some(p) => break p,
                    None => {
                        header = e.next_header.borrow_mut();
                        continue;
                    }
                },
                None => {
                    break null_mut::<u8>();
                }
            }
        }
    }
    pub fn init_with_mmap(&self, memory_map: &MemoryMapHolder) {
        println!("Using mmap at {:#p}", memory_map);
        println!("Loader Info:");
        for e in memory_map.iter() {
            if e.memory_type != EfiMemoryType::LOADER_CODE
                && e.memory_type != EfiMemoryType::LOADER_DATA
            {
                println!("{:?}", e);
                continue;
            }
            println!("{:?}", e);
        }
        println!("Available memory:");
        let mut total_pages = 0;
        for e in memory_map.iter() {
            if e.memory_type != EfiMemoryType::CONVENTIONAL_MEMORY {
                continue;
            }
            println!("{:?}", e);
            self.add_free_from_descriptor(e);
            total_pages += e.number_of_pages;
        }
        println!(
            "Allocator initialized. Total memory: {} MiB",
            total_pages * 4096 / 1024 / 1024
        );
    }
    fn add_free_from_descriptor(&self, desc: &EfiMemoryDescriptor) {
        let mut header = unsafe { Header::new_from_addr(desc.physical_start as usize) };
        header.next_header = None;
        header.is_allocated = false;
        header.size = desc.number_of_pages as usize * 4096;
        let mut first_header = self.first_header.borrow_mut();
        let prev_last = first_header.replace(header);
        drop(first_header);
        let mut header = self.first_header.borrow_mut();
        header.as_mut().unwrap().next_header = prev_last;
        // It's okay not to be sorted the headers at this point
        // since all the regions written in memory maps are not contiguous
        // so that they can't be merged anyway
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

#[test_case]
fn malloc_iterate_free_and_alloc() {
    use alloc::vec::Vec;
    for i in 0..1000 {
        let mut vec = Vec::new();
        vec.resize(i, 10);
        // vec will be deallocatad at the end of this scope
    }
}

#[test_case]
fn malloc_align() {
    let mut pointers = [null_mut::<u8>(); 100];
    for align in [1, 2, 4, 8, 16, 32, 4096] {
        println!("trying align = {}", align);
        for e in pointers.iter_mut() {
            *e = ALLOCATOR.alloc_with_options(
                Layout::from_size_align(1234, align).expect("Failed to create Layout"),
            );
            assert!(*e as usize != 0);
            assert!((*e as usize) % align == 0);
        }
    }
}

#[test_case]
fn malloc_align_random_order() {
    for align in [32, 4096, 8, 4, 16, 2, 1] {
        let mut pointers = [null_mut::<u8>(); 100];
        println!("trying align = {}", align);
        for e in pointers.iter_mut() {
            *e = ALLOCATOR.alloc_with_options(
                Layout::from_size_align(1234, align).expect("Failed to create Layout"),
            );
            assert!(*e as usize != 0);
            assert!((*e as usize) % align == 0);
        }
    }
}

#[test_case]
fn allocated_objects_have_no_overlap() {
    let allocations = [
        Layout::from_size_align(128, 128).unwrap(),
        Layout::from_size_align(32, 32).unwrap(),
        Layout::from_size_align(8, 8).unwrap(),
        Layout::from_size_align(16, 16).unwrap(),
        Layout::from_size_align(6000, 64).unwrap(),
        Layout::from_size_align(4, 4).unwrap(),
        Layout::from_size_align(2, 2).unwrap(),
        Layout::from_size_align(600000, 64).unwrap(),
        Layout::from_size_align(64, 64).unwrap(),
        Layout::from_size_align(1, 1).unwrap(),
        Layout::from_size_align(6000, 64).unwrap(),
        Layout::from_size_align(6000, 64).unwrap(),
        Layout::from_size_align(6000, 64).unwrap(),
        Layout::from_size_align(6000, 64).unwrap(),
        Layout::from_size_align(6000, 64).unwrap(),
        Layout::from_size_align(6000, 64).unwrap(),
        Layout::from_size_align(3, 64).unwrap(),
        Layout::from_size_align(3, 64).unwrap(),
        Layout::from_size_align(3, 64).unwrap(),
        Layout::from_size_align(3, 64).unwrap(),
        Layout::from_size_align(3, 64).unwrap(),
        Layout::from_size_align(3, 64).unwrap(),
        Layout::from_size_align(3, 64).unwrap(),
        Layout::from_size_align(3, 64).unwrap(),
        Layout::from_size_align(3, 64).unwrap(),
        Layout::from_size_align(3, 64).unwrap(),
        Layout::from_size_align(6000, 64).unwrap(),
        Layout::from_size_align(6000, 64).unwrap(),
        Layout::from_size_align(600000, 64).unwrap(),
        Layout::from_size_align(6000, 64).unwrap(),
        Layout::from_size_align(60000, 64).unwrap(),
        Layout::from_size_align(60000, 64).unwrap(),
        Layout::from_size_align(60000, 64).unwrap(),
        Layout::from_size_align(60000, 64).unwrap(),
    ];
    let mut pointers = vec![null_mut::<u8>(); allocations.len()];
    for e in allocations.iter().zip(pointers.iter_mut()).enumerate() {
        let (i, (layout, pointer)) = e;
        println!("alloc layout = {:?}", layout);
        *pointer = ALLOCATOR.alloc_with_options(*layout);
        println!(
            "allocated [{:#018X}, {:#018X})",
            *pointer as usize,
            *pointer as usize + layout.size()
        );
        for k in 0..layout.size() {
            unsafe { *pointer.add(k) = i as u8 }
        }
    }
    for e in allocations.iter().zip(pointers.iter_mut()).enumerate() {
        let (i, (layout, pointer)) = e;
        for k in 0..layout.size() {
            assert!(unsafe { *pointer.add(k) } == i as u8);
        }
    }
    for e in allocations
        .iter()
        .zip(pointers.iter_mut())
        .enumerate()
        .step_by(2)
    {
        let (_, (layout, pointer)) = e;
        unsafe { ALLOCATOR.dealloc(*pointer, *layout) }
    }
    for e in allocations
        .iter()
        .zip(pointers.iter_mut())
        .enumerate()
        .skip(1)
        .step_by(2)
    {
        let (i, (layout, pointer)) = e;
        for k in 0..layout.size() {
            assert!(unsafe { *pointer.add(k) } == i as u8);
        }
    }
    for e in allocations
        .iter()
        .zip(pointers.iter_mut())
        .enumerate()
        .step_by(2)
    {
        let (i, (layout, pointer)) = e;
        println!("alloc layout = {:?}", layout);
        *pointer = ALLOCATOR.alloc_with_options(*layout);
        println!(
            "allocated [{:#018X}, {:#018X})",
            *pointer as usize,
            *pointer as usize + layout.size()
        );
        for k in 0..layout.size() {
            unsafe { *pointer.add(k) = i as u8 }
        }
    }
    for e in allocations.iter().zip(pointers.iter_mut()).enumerate() {
        let (i, (layout, pointer)) = e;
        for k in 0..layout.size() {
            assert!(unsafe { *pointer.add(k) } == i as u8);
        }
    }
}
