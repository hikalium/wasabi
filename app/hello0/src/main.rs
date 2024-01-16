#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

use noli::entry_point;
use noli::sys::exit;
use noli::sys::write_string;

fn main() {
    write_string("**** Hello from an app!\n");
    exit(42);
}

entry_point!(main);
