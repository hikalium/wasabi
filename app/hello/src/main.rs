// Needed to solve: can't find crate for `std`
#![no_std]
#![feature(start)]

// Needed for implementing panic()
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unimplemented!();
}

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
    loop {}
}
