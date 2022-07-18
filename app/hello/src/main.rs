// Needed to solve: can't find crate for `std`
#![no_std]
#![no_main]
#![feature(start)]

// Needed for implementing panic()
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unimplemented!();
}

#[no_mangle]
fn entry() -> isize {
    return -42;
}
