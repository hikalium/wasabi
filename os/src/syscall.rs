use crate::boot_info::BootInfo;
use crate::graphics::draw_point;
use crate::input::InputManager;
use crate::print;
use crate::println;
use crate::x86_64::syscall::return_to_os;
use crate::x86_64::syscall::write_exit_reason;
use crate::x86_64::syscall::write_return_value;
use crate::x86_64::syscall::write_return_value_to_app;

fn exit_to_os(retv: u64) -> ! {
    write_exit_reason(0);
    write_return_value(retv);
    return_to_os();
    unreachable!("Somehow returned from the OS unexpectedly...");
}

// yield_to_os resumes app's execution directly so it will not come back to this code...
fn yield_to_os(retv: u64) -> ! {
    write_exit_reason(1);
    write_return_value(retv);
    return_to_os();
    unreachable!("Somehow returned from the OS unexpectedly...");
}

fn sys_exit(args: &[u64; 5]) -> ! {
    exit_to_os(args[0]);
}

fn sys_print(args: &[u64; 5]) -> u64 {
    // TODO(hikalium): validate the buffer
    let s = args[0] as *const u8;
    let len = args[1] as usize;
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(s, len)) };

    print!("{}", s);
    0
}

fn sys_noop(_args: &[u64; 5]) -> u64 {
    0
}

fn sys_draw_point(args: &[u64; 5]) -> u64 {
    let mut vram = BootInfo::take().vram();
    let x = args[0] as i64;
    let y = args[1] as i64;
    let c = args[2] as u32;
    let result = draw_point(&mut vram, c, x, y);
    if result.is_err() {
        1
    } else {
        0
    }
}

fn sys_read_key(_args: &[u64; 5]) -> u64 {
    if let Some(c) = InputManager::take().pop_input() {
        c as u64
    } else {
        write_return_value_to_app(0);
        yield_to_os(1);
    }
}

pub fn syscall_handler(op: u64, args: &[u64; 5]) -> u64 {
    match op {
        0 => sys_exit(args),
        1 => sys_print(args),
        2 => sys_draw_point(args),
        3 => sys_noop(args),
        4 => sys_read_key(args),
        op => {
            println!("syscall: unimplemented syscall: {}", op);
            1
        }
    }
}
