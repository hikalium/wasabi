#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(os::test_runner::test_runner)]
#![feature(sync_unsafe_cell)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;
use core::pin::Pin;
use core::str::FromStr;
use os::boot_info::BootInfo;
use os::debug_exit;
use os::efi::EfiFileName;
use os::elf::Elf;
use os::error::Error;
use os::error::Result;
use os::executor::yield_execution;
use os::executor::Executor;
use os::executor::Task;
use os::executor::TimeoutFuture;
use os::executor::ROOT_EXECUTOR;
use os::graphics::draw_line;
use os::graphics::BitmapImageBuffer;
use os::init;
use os::input::InputManager;
use os::net::icmp::IcmpPacket;
use os::net::ip::IpV4Addr;
use os::net::network_manager_thread;
use os::net::Network;
use os::print;
use os::println;
use os::ps2::keyboard_task;
use os::serial::SerialPort;
use os::util::Sliceable;
use os::x86_64;
use os::x86_64::init_syscall;
use os::x86_64::paging::write_cr3;
use os::x86_64::read_rsp;

fn paint_wasabi_logo() {
    const SIZE: i64 = 256;
    const COL_SABI: u32 = 0xe33b26;
    const COL_WASABI: u32 = 0x7ec288;

    let mut vram = BootInfo::take().vram();
    let dx = vram.width() / 2 - SIZE;
    let dy = vram.height() / 2 - SIZE;

    // Sabi (Ferris)
    for x in 0..SIZE {
        draw_line(
            &mut vram,
            COL_SABI,
            dx + SIZE,
            dy,
            dx + SIZE / 2 + x,
            dy + SIZE,
        )
        .unwrap();
    }
    // Wasabi
    for x in 0..SIZE {
        draw_line(&mut vram, COL_WASABI, dx, dy, dx + SIZE / 2 + x, dy + SIZE).unwrap();
    }
    for x in 0..SIZE {
        draw_line(
            &mut vram,
            COL_WASABI + 0x3d3d3d,
            dx + SIZE * 2,
            dy,
            dx + SIZE / 2 + x,
            dy + SIZE,
        )
        .unwrap();
    }
}

fn run_command(cmdline: &str) -> Result<()> {
    let network = Network::take();
    let args = cmdline.trim();
    let args: Vec<&str> = args.split(' ').collect();
    println!("\n{args:?}");
    if let Some(&cmd) = args.first() {
        match cmd {
            "panic" => unsafe {
                asm!("int3");
            },
            "ip" => {
                println!("netmask: {:?}", network.netmask());
                println!("router: {:?}", network.router());
                println!("dns: {:?}", network.dns());
            }
            "ping" => {
                if let Some(ip) = args.get(1) {
                    let ip = IpV4Addr::from_str(ip);
                    if let Ok(ip) = ip {
                        network.send_ip_packet(IcmpPacket::new_request(ip).copy_into_slice());
                    } else {
                        println!("{ip:?}")
                    }
                } else {
                    println!("usage: ip <target_ipv4_addr>")
                }
            }
            "arp" => {
                println!("{:?}", network.arp_table_cloned())
            }
            app_name => {
                let result = run_app(app_name);
                println!("{result:?}");
            }
        }
    }
    Ok(())
}

fn run_tasks() -> Result<()> {
    let task0 = async {
        let mut vram = BootInfo::take().vram();
        let h = 10;
        let colors = [0xFF0000, 0x00FF00, 0x0000FF];
        let y = vram.height() / 16 * 14;
        let xbegin = vram.width() / 2;
        let mut x = xbegin;
        let mut c = 0;
        loop {
            draw_line(&mut vram, colors[c % 3], x, y, x, y + h)?;
            x += 1;
            if x >= vram.width() {
                x = xbegin;
                c += 1;
            }
            TimeoutFuture::new_ms(10).await;
            yield_execution().await;
        }
    };
    let task1 = async {
        let mut vram = BootInfo::take().vram();
        let h = 10;
        let colors = [0xFF0000, 0x00FF00, 0x0000FF];
        let y = vram.height() / 16 * 15;
        let xbegin = vram.width() / 2;
        let mut x = xbegin;
        let mut c = 0;
        loop {
            draw_line(&mut vram, colors[c % 3], x, y, x, y + h)?;
            x += 1;
            if x >= vram.width() {
                x = xbegin;
                c += 1;
            }
            TimeoutFuture::new_ms(20).await;
            yield_execution().await;
        }
    };
    let serial_task = async {
        let sp = SerialPort::default();
        loop {
            if let Some(c) = sp.try_read() {
                if let Some(c) = char::from_u32(c as u32) {
                    InputManager::take().push_input(c);
                }
            }
            TimeoutFuture::new_ms(20).await;
            yield_execution().await;
        }
    };
    let console_task = async {
        let mut s = String::new();
        loop {
            if let Some(c) = InputManager::take().pop_input() {
                if c == '\r' || c == '\n' {
                    if let Err(e) = run_command(&s) {
                        println!("{e:?}");
                    };
                    s.clear();
                }
                match c {
                    'a'..='z' | 'A'..='Z' | '0'..='9' | ' ' | '.' => {
                        print!("{c}");
                        s.push(c);
                    }
                    c if c as u8 == 0x7f => {
                        print!("{0} {0}", 0x08 as char);
                        s.pop();
                    }
                    _ => {
                        // Do nothing
                    }
                }
            }
            TimeoutFuture::new_ms(20).await;
            yield_execution().await;
        }
    };
    // This is safe since GlobalAllocator is already initialized.
    {
        let mut executor = ROOT_EXECUTOR.lock();
        executor.spawn(Task::new(task0));
        executor.spawn(Task::new(task1));
        executor.spawn(Task::new(async { keyboard_task().await }));
        executor.spawn(Task::new(serial_task));
        executor.spawn(Task::new(console_task));
        if false {
            executor.spawn(Task::new(async { network_manager_thread().await }));
        }
    }
    init::init_pci();
    Executor::run(&ROOT_EXECUTOR);
    Ok(())
}

fn run_app(name: &str) -> Result<i64> {
    let boot_info = BootInfo::take();
    let root_files = boot_info.root_files();
    let root_files: alloc::vec::Vec<&os::boot_info::File> =
        root_files.iter().filter_map(|e| e.as_ref()).collect();
    let name = EfiFileName::from_str(name)?;
    let elf = root_files.iter().find(|&e| e.name() == &name);
    if let Some(elf) = elf {
        let elf = Elf::parse(elf)?;
        let app = elf.load()?;
        let result = app.exec()?;
        #[cfg(test)]
        debug_exit::exit_qemu(debug_exit::QemuExitCode::Success);
        #[cfg(not(test))]
        Ok(result)
    } else {
        Err(Error::Failed("Init app file not found"))
    }
}

fn main() -> Result<()> {
    println!("Booting WasabiOS...");
    println!("Wasabi OS booted. efi_main = {:#p}", efi_main as *const ());
    init::init_graphical_terminal();
    paint_wasabi_logo();

    unsafe { core::arch::asm!("cli") }
    let interrupt_config = init::init_interrupts()?;
    core::mem::forget(interrupt_config);
    init::init_paging()?;
    init::init_timer();
    init_syscall();

    println!(
        "Welcome to WasabiOS! efi_main = {:#018p}",
        efi_main as *const ()
    );
    println!("debug_info: write_cr3 = {:#018p}", write_cr3 as *const ());

    let boot_info = BootInfo::take();
    let root_files = boot_info.root_files();
    let root_files: alloc::vec::Vec<&os::boot_info::File> =
        root_files.iter().filter_map(|e| e.as_ref()).collect();
    let init_app = EfiFileName::from_str("init.txt")?;
    let init_app = root_files.iter().find(|&e| e.name() == &init_app);
    if let Some(init_app) = init_app {
        let init_app = String::from_utf8_lossy(init_app.data());
        let init_app = init_app.trim();
        let init_app = EfiFileName::from_str(init_app)?;
        let elf = root_files.iter().find(|&e| e.name() == &init_app);
        if let Some(elf) = elf {
            let elf = Elf::parse(elf)?;
            let app = elf.load()?;
            app.exec()?;
            debug_exit::exit_qemu(debug_exit::QemuExitCode::Success);
        } else {
            return Err(Error::Failed("Init app file not found"));
        }
    }

    run_tasks()?;
    Ok(())
}

#[no_mangle]
fn stack_switched() -> ! {
    println!("rsp switched to: {:#018X}", read_rsp());
    // For normal boot
    #[cfg(not(test))]
    main().unwrap();
    // For unit tests in main.rs
    #[cfg(test)]
    test_main();

    x86_64::rest_in_peace()
}

#[no_mangle]
fn efi_main(
    image_handle: os::efi::EfiHandle,
    efi_system_table: Pin<&'static os::efi::EfiSystemTable>,
) {
    os::init::init_basic_runtime(image_handle, efi_system_table);
    println!("rsp on boot: {:#018X}", read_rsp());
    let new_rsp = BootInfo::take().kernel_stack().as_ptr() as usize + os::init::KERNEL_STACK_SIZE;
    unsafe { x86_64::switch_rsp(new_rsp as u64, stack_switched) }
}
