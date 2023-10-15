#![cfg_attr(not(test), no_std)]
#![feature(core_intrinsics)]

mod font;

use crate::font::BITMAP_FONT;
use core::arch::asm;
use core::fmt;
use core::panic::PanicInfo;

// See os/src/x86_64.rs for the calling conventions
pub fn syscall_0(func: u64) -> u64 {
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
pub fn syscall_1(func: u64, arg1: u64) -> u64 {
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
pub fn syscall_2(func: u64, arg1: u64, arg2: u64) -> u64 {
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
pub fn syscall_3(func: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
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
pub fn syscall_4(func: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> u64 {
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
        in("r9") arg4,
        out("r11") _, // destroyed by the syscall instruction
        )
    }
    retv
}
pub fn sys_exit(code: u64) -> ! {
    syscall_1(0, code);
    unreachable!()
}
pub fn sys_print(s: &str) -> u64 {
    let len = s.len() as u64;
    let s = s.as_ptr() as u64;
    syscall_2(1, s, len)
}
pub fn sys_draw_point(x: i64, y: i64, c: u32) -> u64 {
    syscall_3(2, x as u64, y as u64, c as u64)
}

pub fn sys_noop() -> u64 {
    syscall_0(3)
}

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

/// Draws string in one line. New lines are ignored.
pub fn draw_string(color: u32, x: i64, y: i64, s: &str) -> Result<(), ()> {
    let mut pos = 0;
    for c in s.chars() {
        draw_char(color, x + pos, y, c)?;
        pos += 9;
    }
    Ok(())
}

/// Draws a character to the position of `x` and `y`. Upper case characters, lower case characters
/// and symbols are supported.
pub fn draw_char(color: u32, x: i64, y: i64, c: char) -> Result<(), ()> {
    let font_data = BITMAP_FONT[c as usize];
    for i in 0..font_data.len() {
        for j in 0..8 {
            if (font_data[i] >> j) & 0b1 == 0b1 {
                draw_point(color, x + j, y + i as i64)?;
            }
        }
    }
    Ok(())
}

pub fn fill_circle(color: u32, center_x: i64, center_y: i64, radius: i64) -> Result<(), ()> {
    for i in 0..radius * 2 + 1 {
        for j in 0..radius * 2 + 1 {
            let x = i - radius;
            let y = j - radius;

            if x * x + y * y <= radius * radius + 1 {
                draw_point(color, i + center_x, j + center_y)?;
            }
        }
    }
    Ok(())
}

pub fn draw_rect(color: u32, x: i64, y: i64, width: i64, height: i64) -> Result<(), ()> {
    draw_line(color, x, y, x + width, y)?;
    draw_line(color, x, y, x, y + height)?;
    draw_line(color, x + width, y, x + width, y + height)?;
    draw_line(color, x, y + height, x + width, y + height)?;
    Ok(())
}

pub fn draw_line(color: u32, x0: i64, y0: i64, x1: i64, y1: i64) -> Result<(), ()> {
    if x1 < x0 {
        return draw_line(color, x1, y1, x0, y0);
    }
    if x1 == x0 {
        if y0 <= y1 {
            for i in y0..=y1 {
                draw_point(color, x0, i)?;
            }
        } else {
            for i in y1..=y0 {
                draw_point(color, x0, i)?;
            }
        }
        return Ok(());
    }
    assert!(x0 < x1);
    let lx = x1 - x0 + 1;
    const MULTIPLIER: i64 = 1024 * 1024;
    let a = (y1 - y0) * MULTIPLIER / lx;
    for i in 0..lx {
        draw_line(
            color,
            x0 + i,
            y0 + (a * i / MULTIPLIER),
            x0 + i,
            y0 + (a * (i + 1) / MULTIPLIER),
        )?;
    }
    draw_point(color, x0, y0)?;
    draw_point(color, x1, y1)?;
    Ok(())
}

pub fn draw_point(c: u32, x: i64, y: i64) -> Result<(), ()> {
    let result = sys_draw_point(x, y, c);
    if result == 0 {
        Ok(())
    } else {
        Err(())
    }
}

#[macro_export]
macro_rules! entry_point {
    // c.f. https://docs.rs/bootloader/0.6.4/bootloader/macro.entry_point.html
    ($path:path) => {
        #[no_mangle]
        pub unsafe extern "C" fn entry() -> ! {
            // validate the signature of the program entry point
            let f: fn() -> u64 = $path;
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
