extern crate alloc;

use crate::efi::EfiMemoryDescriptor;
use crate::efi::EfiMemoryType;
use crate::memory_map_holder::MemoryMapHolder;
use crate::println;
use alloc::alloc::GlobalAlloc;
use alloc::alloc::Layout;
use alloc::boxed::Box;
use core::borrow::BorrowMut;
use core::cell::RefCell;
use core::ops::DerefMut;

/*
macro_rules! log_malloc {
        () => ($crate::print!("\n"));
            ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
*/
macro_rules! log_malloc {
        () => ($crate::print_nothing!("\n"));
            ($($arg:tt)*) => ($crate::print_nothing!("{}\n", format_args!($($arg)*)));
}

/*
<- free --------------------><- used --><- free ---------------->
|- FreeInfo ---|- unused ---|           |- FreeInfo ---|--------|
^ &FreeInfo                             ^ &FreeInfo
<- bytes ------------------->           <- ---------bytes ------>
|--next_free_info --------------------->|
*/
#[derive(Debug)]
struct Header {
    next_header: Option<Box<Header>>,
    size: usize,
    is_allocated: bool,
}
impl Header {
    fn can_provide(&self, size: usize) -> bool {
        self.size >= size + Self::header_size() * 2
    }
    fn is_allocated(&self) -> bool {
        self.is_allocated
    }
    fn header_size() -> usize {
        core::mem::size_of::<Self>()
    }
    fn size(&self) -> usize {
        self.size
    }
    fn body_addr(&self) -> usize {
        self as *const Header as usize + Self::header_size()
    }
    fn end_addr(&self) -> usize {
        self as *const Header as usize + self.size()
    }
    unsafe fn new_from_addr(addr: usize) -> Box<Header> {
        let header = addr as *mut Header;
        header.write(Header {
            next_header: None,
            size: 0,
            is_allocated: false,
        });
        alloc::boxed::Box::from_raw(addr as *mut Header)
    }
    unsafe fn from_allocated_region(addr: *mut u8) -> Box<Header> {
        let header = addr.sub(Self::header_size()) as *mut Header;
        alloc::boxed::Box::from_raw(header)
    }
    fn provide(&mut self, size: usize) -> Option<*mut u8> {
        if self.is_allocated() || !self.can_provide(size) {
            None
        } else {
            let mut header_for_allocated =
                unsafe { Self::new_from_addr(self.end_addr() - size - Self::header_size()) };
            header_for_allocated.is_allocated = true;
            header_for_allocated.size = size + Self::header_size();
            header_for_allocated.next_header = self.next_header.take();
            self.size -= header_for_allocated.size();
            let addr = header_for_allocated.body_addr();
            self.next_header = Some(header_for_allocated);
            Some(addr as *mut u8)
        }
    }
}
impl Drop for Header {
    fn drop(&mut self) {
        panic!("Header should not be dropped!");
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
        log_malloc!("alloc! {:?}", layout);
        self.alloc_with_options(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        log_malloc!("free! {:#p} {:?}", ptr, layout);
        let mut region = Header::from_allocated_region(ptr);
        region.is_allocated = false;
        Box::leak(region);
        // region is leaked here to avoid dropping the free info on the memory.
    }
}

impl FirstFitAllocator {
    pub fn alloc_with_options(&self, layout: Layout) -> *mut u8 {
        let mut header = self.first_header.borrow_mut();
        let mut header = header.deref_mut();
        loop {
            match header {
                Some(e) => match e.provide((layout.size() + 15) & !15usize) {
                    Some(p) => break p,
                    None => {
                        header = e.next_header.borrow_mut();
                        continue;
                    }
                },
                None => {
                    break core::ptr::null_mut::<u8>();
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
        header.size = (desc.number_of_pages as usize) * 4096;
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
