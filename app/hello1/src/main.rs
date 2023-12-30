#![no_std]
#![no_main]

use noli::entry_point;
use noli::println;
use noli::syscall;

static mut A: u64 = 1;

fn f<F: FnOnce() -> u64>(g: F, c: u64) {
    unsafe {
        A *= g() + c;
    }
}

fn main() -> u64 {
    syscall::print("hello from sys_print!\n");
    f(|| 3, 2);
    f(|| 5, 7);
    println!("heyheyhey!");
    syscall::exit(unsafe { A });
}

entry_point!(main);
