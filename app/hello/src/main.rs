#![no_std]
#![no_main]

use noli::*;

fn main() -> i64 {
    sys_print("Hello, this is Wasabi OS!");
    return -42;
}

entry_point!(main);
