//! System call definitions and its interfaces.
//! See os/src/x86_64.rs for the syscall calling conventions.

use core::arch::asm;

fn syscall_0(func: u64) -> u64 {
    let mut retv;
    unsafe {
        asm!(
        "syscall",
        out("rax") retv,
        out("rcx") _, // destroyed by the syscall instruction
        in("rdx") func,
        out("r11") _, // destroyed by the syscall instruction
        )
    }
    retv
}
fn syscall_1(func: u64, arg1: u64) -> u64 {
    let mut retv;
    unsafe {
        asm!(
        "syscall",
        out("rax") retv,
        out("rcx") _, // destroyed by the syscall instruction
        in("rdx") func,
        in("rsi") arg1,
        out("r11") _, // destroyed by the syscall instruction
        )
    }
    retv
}
fn syscall_2(func: u64, arg1: u64, arg2: u64) -> u64 {
    let mut retv;
    unsafe {
        asm!(
        "syscall",
        out("rax") retv,
        out("rcx") _, // destroyed by the syscall instruction
        in("rdx") func,
        in("rsi") arg1,
        in("rdi") arg2,
        out("r11") _, // destroyed by the syscall instruction
        )
    }
    retv
}
fn syscall_3(func: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    let mut retv;
    unsafe {
        asm!(
        "syscall",
        out("rax") retv,
        out("rcx") _, // destroyed by the syscall instruction
        in("rdx") func,
        in("rsi") arg1,
        in("rdi") arg2,
        in("r8") arg3,
        out("r11") _, // destroyed by the syscall instruction
        )
    }
    retv
}

pub fn exit(code: u64) -> ! {
    syscall_1(0, code);
    unreachable!()
}
pub fn print(s: &str) -> u64 {
    let len = s.len() as u64;
    let s = s.as_ptr() as u64;
    syscall_2(1, s, len)
}
pub fn draw_point(x: i64, y: i64, c: u32) -> u64 {
    syscall_3(2, x as u64, y as u64, c as u64)
}
pub fn noop() -> u64 {
    syscall_0(3)
}
