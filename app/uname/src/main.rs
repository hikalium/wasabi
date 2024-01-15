#![no_std]
#![no_main]

use noli::entry_point;
use noli::sys::write_string;

fn main() -> u64 {
    write_string("Hello hikalium! This is wasabi app!\n");
    0
}
entry_point!(main);
