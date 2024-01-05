#![cfg_attr(not(test), no_std)]
#![feature(alloc_error_handler)]

mod allocator;
mod error;
mod font;
pub mod graphics;
pub mod net;
pub mod syscall;
pub mod window;

use core::fmt;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("PANIC!!!");
    println!("{}", info);
    syscall::exit(1)
}

#[macro_export]
macro_rules! print {
        ($($arg:tt)*) => ($crate::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
        () => ($crate::print!("\n"));
            ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

pub struct StdIoWriter {}
impl StdIoWriter {}
impl fmt::Write for StdIoWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        syscall::print(s);
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut writer = crate::StdIoWriter {};
    fmt::write(&mut writer, args).unwrap();
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
            noli::syscall::exit(ret);
        }
    };
}
