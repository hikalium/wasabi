#![no_std]
#![no_main]
use noli::*;
fn main() -> i64 {
    sys_print("Hello hikalium! This is wasabi app!\n");
    0
}
entry_point!(main);
