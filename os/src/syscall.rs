use crate::boot_info::BootInfo;
use crate::graphics::draw_point;
use crate::input::InputManager;
use crate::print;
use crate::println;
use crate::x86_64::ExecutionContext;
use crate::x86_64::CONTEXT_OS;

fn write_return_value(retv: u64) {
    CONTEXT_OS.lock().cpu.rax = retv;
}

fn write_exit_reason(retv: u64) {
    CONTEXT_OS.lock().cpu.r8 = retv;
}

fn return_to_os() {
    let return_to = CONTEXT_OS.lock().cpu.rip;
    // SAFETY: This is safe as far as the CONTEXT_OS is valid so that
    // we can return to the OS world correctly.
    unsafe {
        let ctx = CONTEXT_OS.lock().as_mut_ptr();
        // c.f. https://rust-lang.github.io/unsafe-code-guidelines/layout/function-pointers.html
        let f: extern "sysv64" fn(*const ExecutionContext) -> ! = core::mem::transmute(return_to);
        f(ctx)
    }
}

fn exit_to_os(retv: u64) -> ! {
    write_exit_reason(0);
    write_return_value(retv);
    return_to_os();
    unreachable!("Somehow returned from the OS unexpectedly...");
}

fn yield_to_os(retv: u64) {
    write_exit_reason(1);
    write_return_value(retv);
    return_to_os();
}

fn sys_exit(regs: &[u64; 7]) -> ! {
    exit_to_os(regs[1]);
}

fn sys_print(args: &[u64; 7]) -> u64 {
    // TODO(hikalium): validate the buffer
    let s = args[1] as *const u8;
    let len = args[2] as usize;
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(s, len)) };

    print!("{}", s);
    0
}

fn sys_noop(_args: &[u64; 7]) -> u64 {
    0
}

fn sys_draw_point(args: &[u64; 7]) -> u64 {
    let mut vram = BootInfo::take().vram();
    let x = args[1] as i64;
    let y = args[2] as i64;
    let c = args[3] as u32;
    let result = draw_point(&mut vram, c, x, y);
    if result.is_err() {
        1
    } else {
        0
    }
}

fn sys_read_key(_args: &[u64; 7]) -> u64 {
    (loop {
        if let Some(c) = InputManager::take().pop_input() {
            break c;
        }
        yield_to_os(1);
    }) as u64
}

#[no_mangle]
pub extern "sysv64" fn syscall_handler(regs: &mut [u64; 16]) {
    let args = {
        let mut args = [0u64; 7];
        args.copy_from_slice(&regs[1..8]);
        args
    };
    let op = args[0];
    let ret = match op {
        0 => sys_exit(&args),
        1 => sys_print(&args),
        2 => sys_draw_point(&args),
        3 => sys_noop(&args),
        4 => sys_read_key(&args),
        op => {
            println!("syscall: unimplemented syscall: {}", op);
            1
        }
    };
    regs[0] = ret;
}
