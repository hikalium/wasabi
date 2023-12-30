#![no_std]
#![no_main]

use noli::entry_point;
use noli::syscall;

fn main() -> u64 {
    syscall::print("**** Hello from an app!\n");
    syscall::exit(42);
}

entry_point!(main);
