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
use noli::net::IpV4Addr;
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

/// sys_nslookup provides DNS resolution for applications.
/// As written in [RFC2606](https://datatracker.ietf.org/doc/html/rfc2606#section-2),
/// this function handles some hard-coded hostnames for testing purpose.
fn sys_nslookup(args: &[u64; 5]) -> i64 {
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
    } else if host == "host.test" {
        // Host (=default gateway) in the QEMU user network.
        // The host machine's exposed ports will be accessible via this address.
        // It also responds to ICMP ping request.
        result[0] = [10, 0, 2, 2];
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

fn sys_tcp_connect(args: &[u64; 5]) -> i64 {
    let ip = IpV4Addr::new((args[0] as u32).to_be_bytes());
    let port: u16 = args[1] as u16;

    if let Some(proc) = CURRENT_PROCESS.lock().as_mut() {
        if let Ok(handle) = proc.create_tcp_socket(ip, port) {
            handle
        } else {
            -1
        }
    } else {
        -1
    }
}

fn sys_tcp_write(args: &[u64; 5]) -> i64 {
    let handle = args[0] as i64;
    let buf = {
        let buf = args[1] as *const u8;
        let len = args[2] as usize;
        // TODO(hikalium): validate the buffer
        unsafe { core::slice::from_raw_parts(buf, len) }
    };
    let sock = if let Some(proc) = CURRENT_PROCESS.lock().as_mut() {
        if let Some(sock) = proc.tcp_socket(handle) {
            Ok(sock)
        } else {
            Err(-1)
        }
    } else {
        Err(-1)
    };
    match sock {
        Ok(sock) => {
            sock.tx_data().lock().extend(buf.iter());
            // Flush the tx buffer
            // TODO(hikalium): remove this (or make the flush operation optional) once preemptive
            // multi-tasking is implemented.
            info!("tx data enqueued. waiting...");
            while sock.is_trying_to_connect()
                || (sock.is_established() && sock.tx_data().lock().len() != 0)
            {
                Scheduler::root().switch_process();
            }
            info!("write done");
            buf.len() as i64
        }
        Err(e) => e,
    }
}

fn sys_tcp_read(args: &[u64; 5]) -> i64 {
    let handle = args[0] as i64;
    let buf = {
        let buf = args[1] as *mut u8;
        let len = args[2] as usize;
        // TODO(hikalium): validate the buffer
        unsafe { core::slice::from_raw_parts_mut(buf, len) }
    };
    let sock = if let Some(proc) = CURRENT_PROCESS.lock().as_mut() {
        if let Some(sock) = proc.tcp_socket(handle) {
            Ok(sock)
        } else {
            Err(-1)
        }
    } else {
        Err(-1)
    };
    match sock {
        Ok(sock) => {
            while sock.is_trying_to_connect()
                || (sock.is_established() && sock.rx_data().lock().len() == 0)
            {
                Scheduler::root().switch_process();
            }
            let mut rx_data_locked = sock.rx_data().lock();
            let src_buf_size = rx_data_locked.len();
            let dst_buf_size = buf.len();
            let mut received = 0;
            for (dst, src) in buf
                .iter_mut()
                .zip(rx_data_locked.drain(0..(core::cmp::min(dst_buf_size, src_buf_size))))
            {
                *dst = src;
                received += 1;
            }
            received
        }
        Err(e) => e,
    }
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
        8 => sys_tcp_connect(args) as u64,
        9 => sys_tcp_write(args) as u64,
        10 => sys_tcp_read(args) as u64,
        op => {
            println!("syscall: unimplemented syscall: {}", op);
            // Return u64::MAX here as it may be the "most unexpected value" that can crash the
            // program without keep going. For example, most of the syscalls uses negative values
            // as an "error" value. Also, even if the value is treated as unsigned size of
            // something, subsequent operations can fail with such a huge number anyways (e.g.
            // allocating a buffer) so that the software developer can notice the issue easily
            // (hopefully...)
            u64::MAX
        }
    }
}
