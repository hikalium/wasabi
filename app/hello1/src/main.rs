#![no_std]
#![no_main]

use core::arch::asm;
use noli::*;

static mut A: i64 = 1;

fn f<F: FnOnce() -> i64>(g: F, c: i64) {
    unsafe {
        A *= g() + c;
    }
}

fn main() -> i64 {
    f(|| 3, 2);
    f(|| 5, 7);
    println!("heyheyhey!");
    unsafe {
        asm!("int3");
    }
    sys_exit(unsafe { A });
}

entry_point!(main);
