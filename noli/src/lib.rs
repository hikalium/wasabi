#![cfg_attr(not(test), no_std)]
//#![feature(alloc_error_handler)]
#![feature(core_intrinsics)]

//extern crate alloc;

//use alloc::alloc::GlobalAlloc;
//use alloc::alloc::Layout;
use core::arch::asm;
use core::fmt;
use core::panic::PanicInfo;
//use core::ptr::null_mut;

/*
Wasabi OS calling convention:
    args:
        args[0]: rax (syscall number)
        args[1]: rdi
        args[2]: rsi
        args[3]: rdx
        args[4]: r10
        args[5]: r8
        args[6]: r9
    return:
        retv[0]: rax
    scratch: (will be destroyed)
        rcx
*/

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("PANIC!!!");
    println!("{}", info);
    sys_exit(1)
}

#[macro_export]
macro_rules! print {
        ($($arg:tt)*) => (_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
        () => ($crate::print!("\n"));
            ($($arg:tt)*) => (print!("{}\n", format_args!($($arg)*)));
}

pub struct StdIoWriter {}
impl StdIoWriter {}
impl fmt::Write for StdIoWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        sys_print(s);
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut writer = crate::StdIoWriter {};
    fmt::write(&mut writer, args).unwrap();
}

pub fn sys_exit(code: i64) -> ! {
    unsafe {
        asm!(
        "mov rax, 0",
        "syscall",
        out("rcx") _, // will be broken by syscall
        in("rdi") code,
        )
    }
    unreachable!()
}

pub fn sys_print(s: &str) -> i64 {
    let len = s.len();
    let s = s.as_ptr() as u64;
    let mut result;
    unsafe {
        asm!(
        "mov rax, 1",
        "syscall",
        out("rcx") _, // will be broken by syscall
        in("rdi") s,
        in("rsi") len,
        lateout("rax") result);
    }
    result
}

pub fn sys_draw_point() -> u64 {
    let mut result;
    unsafe {
        asm!(
        "mov rax, 2",
        "mov rcx, 0",
        "syscall",
        lateout("rcx") result);
    }
    result
}

pub fn sys_noop() -> u64 {
    let mut result;
    unsafe {
        asm!(
        "mov rax, 3",
        "mov rcx, 0",
        "syscall",
        lateout("rcx") result);
    }
    result
}

#[macro_export]
macro_rules! entry_point {
    // c.f. https://docs.rs/bootloader/0.6.4/bootloader/macro.entry_point.html
    ($path:path) => {
        #[no_mangle]
        pub unsafe extern "C" fn entry() -> ! {
            // validate the signature of the program entry point
            let f: fn() -> i64 = $path;
            let ret = f();
            sys_exit(ret);
        }
    };
}

//trait MutableAllocator {
//    fn alloc(&mut self, layout: Layout) -> *mut u8;
//    fn dealloc(&mut self, _ptr: *mut u8, _layout: Layout);
//}
//
//const ALLOCATOR_BUF_SIZE: usize = 0x10000;
//pub struct WaterMarkAllocator {
//    buf: [u8; ALLOCATOR_BUF_SIZE],
//    used_bytes: usize,
//}
//
//pub struct GlobalAllocatorWrapper {
//    allocator: WaterMarkAllocator,
//}
//
//#[global_allocator]
//static mut ALLOCATOR: GlobalAllocatorWrapper = GlobalAllocatorWrapper {
//    allocator: WaterMarkAllocator {
//        buf: [0; ALLOCATOR_BUF_SIZE],
//        used_bytes: 0,
//    },
//};
//
//#[alloc_error_handler]
//fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
//    panic!("allocation error: {:?}", layout)
//}
//
//impl MutableAllocator for WaterMarkAllocator {
//    fn alloc(&mut self, layout: Layout) -> *mut u8 {
//        if self.used_bytes > ALLOCATOR_BUF_SIZE {
//            return null_mut();
//        }
//        self.used_bytes = (self.used_bytes + layout.align() - 1) / layout.align() * layout.align();
//        self.used_bytes += layout.size();
//        if self.used_bytes > ALLOCATOR_BUF_SIZE {
//            return null_mut();
//        }
//        unsafe { self.buf.as_mut_ptr().add(self.used_bytes - layout.size()) }
//    }
//    fn dealloc(&mut self, _ptr: *mut u8, _layout: Layout) {}
//}
//unsafe impl GlobalAlloc for GlobalAllocatorWrapper {
//    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
//        ALLOCATOR.allocator.alloc(layout)
//    }
//
//    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
//        ALLOCATOR.allocator.dealloc(ptr, layout);
//    }
//}
