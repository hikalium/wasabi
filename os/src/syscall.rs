use crate::print;
use crate::println;
use crate::x86_64::ExecutionContext;
use crate::x86_64::CONTEXT_OS;

fn sys_exit(regs: &[u64; 15]) {
    println!("program exited with code {}", regs[1]);
    {
        let ctx = CONTEXT_OS.lock();
        let ctx = *ctx;
        println!("CONTEXT_OS: {:?}", ctx);
        if ctx.is_null() {
            panic!("context is invalid");
        }
        unsafe {
            println!("{:?}", (*ctx).cpu);
            // c.f. https://rust-lang.github.io/unsafe-code-guidelines/layout/function-pointers.html

            let f: extern "sysv64" fn(*const ExecutionContext) -> ! =
                core::mem::transmute((*ctx).cpu.rip);
            f(ctx)
        }
    }
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
