use crate::prelude::*;

use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::ptr::null_mut;
use core::slice;
use sabi::MouseEvent;
use sabi::RawIpV4Addr;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    crate::println!("PANIC!!!");
    crate::println!("{}", info);
    Api::exit(1)
}

#[macro_export]
macro_rules! entry_point {
    // c.f. https://docs.rs/bootloader/0.6.4/bootloader/macro.entry_point.html
    ($path:path) => {
        #[no_mangle]
        pub unsafe extern "C" fn entry() -> ! {
            // Using [crate::error::MainReturn] to accept multiple return types.
            // c.f. https://github.com/rust-lang/rfcs/issues/1176#issuecomment-115058364
            use noli::prelude::*;
            let ret = $path();
            let code = ret.as_return_code();
            if code != 0 {
                println!("{ret:?}")
            }
            Api::exit(code)
        }
    };
}

trait MutableAllocator {
    fn alloc(&mut self, layout: Layout) -> *mut u8;
    fn dealloc(&mut self, _ptr: *mut u8, _layout: Layout);
}

const ALLOCATOR_BUF_SIZE: usize = 0x240000;
pub struct WaterMarkAllocator {
    buf: [u8; ALLOCATOR_BUF_SIZE],
    used_bytes: usize,
}

pub struct GlobalAllocatorWrapper {
    allocator: WaterMarkAllocator,
}

#[cfg(not(target_os = "linux"))]
#[global_allocator]
static mut ALLOCATOR: GlobalAllocatorWrapper = GlobalAllocatorWrapper {
    allocator: WaterMarkAllocator {
        buf: [0; ALLOCATOR_BUF_SIZE],
        used_bytes: 0,
    },
};

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

// System call definitions and its interfaces.
// See os/src/x86_64.rs for the syscall calling conventions.

use core::arch::asm;

fn syscall_0(func: u64) -> u64 {
    syscall_5(func, 0, 0, 0, 0, 0)
}
fn syscall_1(func: u64, arg1: u64) -> u64 {
    syscall_5(func, arg1, 0, 0, 0, 0)
}
fn syscall_2(func: u64, arg1: u64, arg2: u64) -> u64 {
    syscall_5(func, arg1, arg2, 0, 0, 0)
}
fn syscall_3(func: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    syscall_5(func, arg1, arg2, arg3, 0, 0)
}
fn syscall_4(func: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> u64 {
    syscall_5(func, arg1, arg2, arg3, arg4, 0)
}
fn syscall_5(func: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> u64 {
    let mut retv;
    unsafe {
        asm!(
        "push rsp",
        "and rsp, -16",
        "syscall",
        "pop rsp",
        out("rax") retv,
        out("rcx") _, // destroyed by the syscall instruction
        in("rdx") func,
        in("rsi") arg1,
        in("rdi") arg2,
        in("r8") arg3,
        in("r9") arg4,
        in("r10") arg5,
        out("r11") _, // destroyed by the syscall instruction
        )
    }
    retv
}

pub struct Api;

impl SystemApi for Api {
    fn exit(code: u64) -> ! {
        syscall_1(0, code);
        unreachable!()
    }
    fn write_string(s: &str) -> u64 {
        let len = s.len() as u64;
        let s = s.as_ptr() as u64;
        syscall_2(1, s, len)
    }
    fn draw_point(x: i64, y: i64, c: u32) -> u64 {
        syscall_3(2, x as u64, y as u64, c as u64)
    }
    fn noop() -> u64 {
        syscall_0(3)
    }
    fn read_key() -> Option<char> {
        let c = syscall_0(4);
        if c == 0 {
            None
        } else {
            char::from_u32(c as u32)
        }
    }
    fn get_mouse_cursor_info() -> Option<MouseEvent> {
        let mut e: MouseEvent = MouseEvent::default();
        let ep = &mut e as *mut MouseEvent as u64;
        if syscall_1(5, ep) == 0 {
            Some(e)
        } else {
            None
        }
    }
    fn get_args_region() -> Option<&'static [u8]> {
        let addr = syscall_0(6);
        if addr == 0 {
            None
        } else {
            let addr = addr as *const u8;
            let mut size = [0u8; 8];
            size.copy_from_slice(unsafe { slice::from_raw_parts(addr, 8) });
            let size = usize::from_le_bytes(size);
            Some(unsafe { slice::from_raw_parts(addr, size) })
        }
    }
    fn nslookup(host: &str, result: &mut [RawIpV4Addr]) -> i64 {
        syscall_4(
            7,
            host.as_ptr() as u64,
            host.len() as u64,
            result.as_ptr() as u64,
            result.len() as u64,
        ) as i64
    }
    fn open_tcp_socket(ip: RawIpV4Addr, port: u16) -> i64 {
        syscall_2(8, u32::from_be_bytes(ip) as u64, port as u64) as i64
    }
    fn write_to_tcp_socket(handle: i64, buf: &[u8]) -> i64 {
        syscall_3(9, handle as u64, buf.as_ptr() as u64, buf.len() as u64) as i64
    }
    fn read_from_tcp_socket(handle: i64, buf: &mut [u8]) -> i64 {
        syscall_3(10, handle as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as i64
    }
}
