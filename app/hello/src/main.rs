#![no_std]
#![no_main]

use noli::*;

fn main() -> i64 {
    //sys_print("hello!");
    //println!("{}", 42);
    return 42;
}

entry_point!(main);
