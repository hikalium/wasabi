#![feature(alloc_error_handler)]
#![no_std]
mod allocator;

pub mod error;
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
            // Using this trait to accept multiple return types.
            // c.f. https://github.com/rust-lang/rfcs/issues/1176#issuecomment-115058364
            use $crate::error::MainReturn;
            let ret = $path().into_error_code();
            noli::syscall::exit(ret);
        }
    };
}
