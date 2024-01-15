use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::ptr::null_mut;

trait MutableAllocator {
    fn alloc(&mut self, layout: Layout) -> *mut u8;
    fn dealloc(&mut self, _ptr: *mut u8, _layout: Layout);
}

const ALLOCATOR_BUF_SIZE: usize = 0x10000;
pub struct WaterMarkAllocator {
    buf: [u8; ALLOCATOR_BUF_SIZE],
    used_bytes: usize,
}

pub struct GlobalAllocatorWrapper {
    allocator: WaterMarkAllocator,
}

#[global_allocator]
static mut ALLOCATOR: GlobalAllocatorWrapper = GlobalAllocatorWrapper {
    allocator: WaterMarkAllocator {
        buf: [0; ALLOCATOR_BUF_SIZE],
        used_bytes: 0,
    },
};

#[cfg(not(target_os = "linux"))]
#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

impl MutableAllocator for WaterMarkAllocator {
    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        if self.used_bytes > ALLOCATOR_BUF_SIZE {
            return null_mut();
        }
        self.used_bytes = (self.used_bytes + layout.align() - 1) / layout.align() * layout.align();
        self.used_bytes += layout.size();
        if self.used_bytes > ALLOCATOR_BUF_SIZE {
            return null_mut();
        }
        unsafe { self.buf.as_mut_ptr().add(self.used_bytes - layout.size()) }
    }
    fn dealloc(&mut self, _ptr: *mut u8, _layout: Layout) {}
}

unsafe impl GlobalAlloc for GlobalAllocatorWrapper {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATOR.allocator.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOCATOR.allocator.dealloc(ptr, layout);
    }
}
