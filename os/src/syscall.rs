use crate::print;
use crate::println;

fn sys_exit(_regs: &[u64; 15]) {
    panic!("program exited (sorry going back to os is not yet supported...)")
}

fn sys_print(regs: &[u64; 15]) {
    let s = regs[1] as *const u8;
    let len = regs[2] as usize;
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(s, len)) };

    print!("{}", s)
}

#[no_mangle]
pub extern "sysv64" fn syscall_handler(regs: &[u64; 15] /* rdi */) {
    println!("syscall {:#018X}", regs[0]);
    match regs[0] {
        0 => sys_exit(regs),
        1 => sys_print(regs),
        e => {
            panic!("unimplemented syscall: {}", e)
        }
    }
}
