#![no_std]
#![no_main]

use noli::*;

static mut A: i64 = 1;

fn f<F: FnOnce() -> i64>(g: F) {
    unsafe {
        println!("A");
        A *= g();
    }
}

fn main() -> i64 {
    f(|| 3);
    f(|| 5);
    return unsafe { A };
}

entry_point!(main);
