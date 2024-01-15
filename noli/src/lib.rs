#![feature(alloc_error_handler)]
#![no_std]
mod allocator;

mod error;
mod font;
pub mod graphics;
pub mod net;
pub mod print;
pub mod syscall;
pub mod window;

#[cfg(not(target_os = "linux"))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC!!!");
    println!("{}", info);
    syscall::exit(1)
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
