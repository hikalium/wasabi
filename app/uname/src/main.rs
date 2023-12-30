#![no_std]
#![no_main]
use noli::*;
fn main() -> u64 {
    syscall::print("Hello hikalium! This is wasabi app!\n");
    0
}
entry_point!(main);
