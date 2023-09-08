#![no_std]
#![no_main]

use noli::*;

fn main() -> i64 {
    println!("window0!!!!");
    println!("window0 {}x{}", 32, 64);
    sys_draw_point(100, 100, 0xff0000);
    0
}

entry_point!(main);
