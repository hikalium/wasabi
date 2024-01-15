use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::fmt;
use core::ptr::null_mut;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    crate::println!("PANIC!!!");
    crate::println!("{}", info);
    exit(1)
}

macro_rules! entry_point {
    // c.f. https://docs.rs/bootloader/0.6.4/bootloader/macro.entry_point.html
    ($path:path) => {
        #[no_mangle]
        pub unsafe extern "C" fn entry() -> ! {
            // Using this trait to accept multiple return types.
            // c.f. https://github.com/rust-lang/rfcs/issues/1176#issuecomment-115058364
            use $crate::error::MainReturn;
            let ret = $path().into_error_code();
            crate::sys::wasabi::exit(ret);
        }
    };
}

trait MutableAllocator {
    fn alloc(&mut self, layout: Layout) -> *mut u8;
    fn dealloc(&mut self, _ptr: *mut u8, _layout: Layout);
}

const ALLOCATOR_BUF_SIZE: usize = 0x20000;
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
    let mut retv;
    unsafe {
        asm!(
        "syscall",
        out("rax") retv,
        out("rcx") _, // destroyed by the syscall instruction
        in("rdx") func,
        out("r11") _, // destroyed by the syscall instruction
        )
    }
    retv
}
fn syscall_1(func: u64, arg1: u64) -> u64 {
    let mut retv;
    unsafe {
        asm!(
        "syscall",
        out("rax") retv,
        out("rcx") _, // destroyed by the syscall instruction
        in("rdx") func,
        in("rsi") arg1,
        out("r11") _, // destroyed by the syscall instruction
        )
    }
    retv
}
fn syscall_2(func: u64, arg1: u64, arg2: u64) -> u64 {
    let mut retv;
    unsafe {
        asm!(
        "syscall",
        out("rax") retv,
        out("rcx") _, // destroyed by the syscall instruction
        in("rdx") func,
        in("rsi") arg1,
        in("rdi") arg2,
        out("r11") _, // destroyed by the syscall instruction
        )
    }
    retv
}
fn syscall_3(func: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    let mut retv;
    unsafe {
        asm!(
        "syscall",
        out("rax") retv,
        out("rcx") _, // destroyed by the syscall instruction
        in("rdx") func,
        in("rsi") arg1,
        in("rdi") arg2,
        in("r8") arg3,
        out("r11") _, // destroyed by the syscall instruction
        )
    }
    retv
}

pub fn exit(code: u64) -> ! {
    syscall_1(0, code);
    unreachable!()
}
pub fn print(s: &str) -> u64 {
    let len = s.len() as u64;
    let s = s.as_ptr() as u64;
    syscall_2(1, s, len)
}
pub fn draw_point(x: i64, y: i64, c: u32) -> u64 {
    syscall_3(2, x as u64, y as u64, c as u64)
}
pub fn noop() -> u64 {
    syscall_0(3)
}

macro_rules! print {
        ($($arg:tt)*) => ($crate::sys::wasabi::_print(format_args!($($arg)*)));
}

macro_rules! println {
    // Note: exported macros will be exposed as the crate's root item.
    // c.f. https://doc.rust-lang.org/reference/macros-by-example.html#path-based-scope
        () => ($crate::print!("\n"));
            ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

pub struct StdIoWriter {}
impl StdIoWriter {}
impl fmt::Write for StdIoWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        print(s);
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut writer = StdIoWriter {};
    fmt::write(&mut writer, args).unwrap();
}
