use crate::boot_info::BootInfo;
use crate::error;
use crate::executor::block_on_and_schedule;
use crate::info;
use crate::input::InputManager;
use crate::net::dns::query_dns;
use crate::net::dns::DnsResponseEntry;
use crate::print;
use crate::println;
use crate::process::Scheduler;
use crate::process::CURRENT_PROCESS;
use crate::x86_64::syscall::return_to_os;
use crate::x86_64::syscall::write_exit_reason;
use crate::x86_64::syscall::write_return_value;
use core::ptr::write_volatile;
use noli::bitmap::bitmap_draw_point;
use sabi::MouseEvent;

fn exit_to_os(retv: u64) -> ! {
    write_exit_reason(0);
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
    let result = bitmap_draw_point(&mut vram, c, x, y);
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
        Scheduler::root().switch_process();
        0
    }
}

fn sys_get_mouse_cursor_position(args: &[u64; 5]) -> u64 {
    if let Some(e) = InputManager::take().pop_cursor_input_absolute() {
        unsafe { write_volatile(args[0] as *mut MouseEvent, e) }
        0
    } else {
        Scheduler::root().switch_process();
        1
    }
}

fn sys_get_args_region(_args: &[u64; 5]) -> u64 {
    if let Some(proc) = CURRENT_PROCESS.lock().as_ref() {
        proc.args_region_start_addr().unwrap_or_default() as u64
    } else {
        0
    }
}

fn sys_nslookup(args: &[u64; 5]) -> i64 {
    info!("sys_nslookup!");
    let host = {
        let host = args[0] as *const u8;
        let len = args[1] as usize;
        // TODO(hikalium): validate the buffer
        unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(host, len)) }
    };
    let result = {
        let result = args[2] as *mut sabi::RawIpV4Addr;
        let len = args[3] as usize;
        // TODO(hikalium): validate the buffer
        unsafe { core::slice::from_raw_parts_mut(result, len) }
    };
    if host == "wasabitest.example.com" {
        result[0] = [127, 0, 0, 1];
        return 1;
    } else if host == "wasabitest.example.invalid" {
        // c.f. https://www.rfc-editor.org/rfc/rfc6761.html
        // >  The domain "invalid." and any names falling within ".invalid." are special in the ways listed below.
        // > Users MAY assume that queries for "invalid" names will always return NXDOMAIN responses.
        // > Name resolution APIs and libraries SHOULD recognize "invalid" names as special and SHOULD always return immediate negative responses.
        return -2;
    }
    let r = block_on_and_schedule(query_dns(host));
    if let Ok(r) = &r {
        if let Some(r) = r.first() {
            let DnsResponseEntry::A { name: _, addr } = &r;
            result[0] = addr.bytes();
            return 1;
        } else {
            error!("empty response so return NXDOMAIN");
            return -2;
        }
    } else {
        error!("{r:?}")
    }
    -1
}

pub fn syscall_handler(op: u64, args: &[u64; 5]) -> u64 {
    match op {
        0 => sys_exit(args),
        1 => sys_print(args),
        2 => sys_draw_point(args),
        3 => sys_noop(args),
        4 => sys_read_key(args),
        5 => sys_get_mouse_cursor_position(args),
        6 => sys_get_args_region(args),
        7 => sys_nslookup(args) as u64,
        op => {
            println!("syscall: unimplemented syscall: {}", op);
            1
        }
    }
}
