use crate::boot_info::BootInfo;
use crate::graphics::draw_point;
use crate::print;
use crate::println;
use crate::x86_64::ExecutionContext;
use crate::x86_64::CONTEXT_OS;

fn write_return_value(retv: u64) {
    let ctx = {
        let ctx = CONTEXT_OS.lock();
        *ctx
    };
    if ctx.is_null() {
        panic!("context is invalid");
    }
    unsafe {
        (*ctx).cpu.rax = retv;
    }
}

fn return_to_os() {
    let ctx = {
        let ctx = CONTEXT_OS.lock();
        *ctx
    };
    if ctx.is_null() {
        panic!("context is invalid");
    }
    unsafe {
        // c.f. https://rust-lang.github.io/unsafe-code-guidelines/layout/function-pointers.html
        let f: extern "sysv64" fn(*const ExecutionContext) -> ! =
            core::mem::transmute((*ctx).cpu.rip);
        f(ctx)
    }
}

fn sys_exit(regs: &[u64; 15]) {
    println!("program exited with code {}", regs[1]);
    {
        let retv = regs[1];
        write_return_value(retv);
        return_to_os()
    }
}

fn sys_print(regs: &[u64; 15]) {
    let s = regs[1] as *const u8;
    let len = regs[2] as usize;
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(s, len)) };

    print!("{}", s)
}

fn sys_noop(_args: &[u64; 15]) {}

fn sys_draw_point(regs: &[u64; 15]) {
    let mut vram = BootInfo::take().vram();
    let x = regs[1] as i64;
    let y = regs[2] as i64;
    let c = regs[3] as u32;
    let result = draw_point(&mut vram, c, x, y);
    let retv = if result.is_err() { 1 } else { 0 };
    write_return_value(retv);
}

#[no_mangle]
pub extern "sysv64" fn syscall_handler(regs: &[u64; 15] /* rdi */) {
    /*
        Wasabi OS calling convention:
        args:
            regs[0]: rax (syscall number)
            regs[1]: rdi (First arg)
            regs[2]: rsi
            regs[3]: rdx
            regs[4]: r10
            regs[5]: r8
            regs[6]: r9
        return:
            regs[0]: rax
        scratch: (will be destroyed)
            rcx
    */
    let op = regs[0];
    match op {
        0 => sys_exit(regs),
        1 => sys_print(regs),
        2 => sys_draw_point(regs),
        3 => sys_noop(regs),
        op => {
            println!("syscall: unimplemented syscall: {}", op);
            write_return_value(1);
        }
    }
}
