#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

use noli::entry_point;
use noli::sys::write_string;

fn main() {
    write_string("Hello hikalium! This is wasabi app!\n");
}
entry_point!(main);
