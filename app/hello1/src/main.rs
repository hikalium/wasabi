#![no_std]
#![no_main]

use noli::*;

static mut A: u64 = 1;

fn f<F: FnOnce() -> u64>(g: F, c: u64) {
    unsafe {
        A *= g() + c;
    }
}

fn main() -> u64 {
    sys_print("hello from sys_print!\n");
    f(|| 3, 2);
    f(|| 5, 7);
    println!("heyheyhey!");
    sys_exit(unsafe { A });
}

entry_point!(main);
