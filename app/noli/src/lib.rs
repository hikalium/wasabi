#![no_std]
use core::arch::asm;

pub fn print(_: &str) {
    unsafe { asm!("syscall") }
}
