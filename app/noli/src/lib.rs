#![no_std]
#![feature(core_intrinsics)]
use core::arch::asm;
use core::fmt;
use core::panic::PanicInfo;

/*
Wasabi OS calling convention:
    args[0]: rax
    args[1]: rdi
    args[2]: rsi
    args[3]: rdx
    args[4]: r10
    args[5]: r8
    args[6]: r9
    return: rax
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

pub fn sys_print(s: &str) -> i64 {
    let len = s.len();
    let s = s.as_ptr() as u64;
    let mut result;
    unsafe {
        asm!(
        "mov rax, 1",
        "syscall",
        in("rdi") s,
        in("rsi") len,
        lateout("rax") result);
    }
    result
}

pub fn sys_exit(code: i64) -> ! {
    unsafe {
        asm!(
        "mov rax, 0",
        "syscall",
        in("rdi") code,
        )
    }
    unreachable!()
}

#[macro_export]
macro_rules! entry_point {
    // c.f. https://docs.rs/bootloader/0.6.4/bootloader/macro.entry_point.html
    ($path:path) => {
        #[no_mangle]
        pub unsafe extern "C" fn entry() -> i64 {
            // validate the signature of the program entry point
            let f: fn() -> i64 = $path;
            f()
        }
    };
}
