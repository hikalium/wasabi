extern crate alloc;

use crate::efi::EFIMemoryDescriptor;
use crate::efi::EFIMemoryType;
use crate::memory_map_holder::MemoryMapHolder;
use crate::println;
use crate::serial;
use alloc::alloc::GlobalAlloc;
use alloc::alloc::Layout;
use core::cell::Cell;
use core::fmt::Write;

pub struct SimpleAllocator {
    free_info: Cell<Option<&'static FreeInfo>>,
}

#[global_allocator]
pub static ALLOCATOR: SimpleAllocator = SimpleAllocator {
    free_info: Cell::new(None),
};

unsafe impl Sync for SimpleAllocator {}

unsafe impl GlobalAlloc for SimpleAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut serial_writer = serial::SerialConsoleWriter {};
        let free_info = (*self.free_info.as_ptr()).expect("free_info is None");
        let pages_needed = (layout.size() + 4095) / 4096;
        if pages_needed > *free_info.num_of_pages.as_ptr() {
            panic!("alloc: {:?}: No more memory", layout);
        }
        *free_info.num_of_pages.as_ptr() -= pages_needed;
        if *free_info.num_of_pages.as_ptr() == 0 {
            // Releases the free info since it is empty
            *self.free_info.as_ptr() = *free_info.next_free_info.as_ptr();
        }
        (free_info as *const FreeInfo as *mut u8).add(*free_info.num_of_pages.as_ptr() * 4096)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let pages_being_freed = (layout.size() + 4095) / 4096;
        let info = &mut *(ptr as *mut FreeInfo);
        *info.next_free_info.as_ptr() = *self.free_info.as_ptr();
        *info.num_of_pages.as_ptr() = pages_being_freed;
        *self.free_info.as_ptr() = Some(info);
    }
}

struct FreeInfo {
    next_free_info: Cell<Option<&'static FreeInfo>>,
    num_of_pages: Cell<usize>,
}

impl SimpleAllocator {
    pub fn init_with_mmap(&self, memory_map: &MemoryMapHolder) {
        let mut total_pages = 0;
        for e in memory_map.iter() {
            if e.memory_type != EFIMemoryType::CONVENTIONAL_MEMORY {
                continue;
            }
            ALLOCATOR.set_descriptor(e);
            total_pages += e.number_of_pages;
            println!("{:?}", e);
        }
        println!(
            "Allocator initialized. Total memory: {} MiB",
            total_pages * 4096 / 1024 / 1024
        );
    }
    fn set_descriptor(&self, desc: &EFIMemoryDescriptor) {
        unsafe {
            let info = &mut *(desc.physical_start as *mut FreeInfo);
            *info.next_free_info.as_ptr() = None;
            *info.num_of_pages.as_ptr() = desc.number_of_pages as usize;
            *self.free_info.as_ptr() = Some(info);
        }
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

#[test_case]
fn malloc_iterate_free_and_alloc() {
    use alloc::vec::Vec;
    for i in 0..1_000_000 {
        let mut vec = Vec::new();
        vec.resize(i, 10);
        // vec will be deallocatad at the end of this scope
    }
}
