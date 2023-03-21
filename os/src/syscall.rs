use crate::print;
use crate::println;

fn sys_exit(regs: &[u64; 15]) {
    println!("program exited with code {}", regs[1]);
    todo!("Exit from an app is not yet implemented!");
}

fn sys_print(regs: &[u64; 15]) {
    let s = regs[1] as *const u8;
    let len = regs[2] as usize;
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(s, len)) };

    print!("{}", s)
}
fn sys_noop(_args: &[u64; 15]) {}

#[no_mangle]
pub extern "sysv64" fn syscall_handler(regs: &[u64; 15] /* rdi */) {
    println!("syscall {:#018X}", regs[0]);
    match regs[0] {
        0 => sys_exit(regs),
        1 => sys_print(regs),
        3 => sys_noop(regs),
        e => {
            panic!("unimplemented syscall: {}", e)
        }
    }
}
