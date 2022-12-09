// Needed to solve: can't find crate for `std`
#![no_std]
#![no_main]
#![feature(start)]

// Needed for implementing panic()
use core::panic::PanicInfo;
use noli::print;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unimplemented!();
}

#[no_mangle]
fn entry() -> isize {
    print("Hello, this is Wasabi OS!");
    return -42;
}
