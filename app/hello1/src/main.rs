#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

use noli::prelude::*;

static mut A: u64 = 1;

fn f<F: FnOnce() -> u64>(g: F, c: u64) {
    unsafe {
        A *= g() + c;
    }
}

fn main() {
    Api::write_string("hello from sys_print!\n");
    f(|| 3, 2);
    f(|| 5, 7);
    println!("heyheyhey!");
    Api::exit(unsafe { A });
}

entry_point!(main);
