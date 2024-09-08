#![no_std]
#![no_main]

#[no_mangle]
fn efi_main() {
    //println!("Hello, world!");
    #[allow(clippy::empty_loop)]
    loop {}
}

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    #[allow(clippy::empty_loop)]
    loop {}
}
