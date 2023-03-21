#![no_std]
#![no_main]

use noli::*;

fn main() -> i64 {
    sys_print("**** Hello from an app!\n");
    sys_exit(42);
}

entry_point!(main);
