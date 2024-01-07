#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(os::test_runner::test_runner)]
#![feature(sync_unsafe_cell)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::string::String;
use core::pin::Pin;
use core::str::FromStr;
use os::boot_info::BootInfo;
use os::debug_exit;
use os::efi::types::EfiHandle;
use os::efi::EfiFileName;
use os::elf::Elf;
use os::error::Error;
use os::error::Result;
use os::executor::spawn_global;
use os::executor::yield_execution;
use os::executor::Executor;
use os::executor::TimeoutFuture;
use os::executor::ROOT_EXECUTOR;
use os::graphics::draw_line;
use os::graphics::Bitmap;
use os::init;
use os::input::enqueue_input_tasks;
use os::println;
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
    // Enqueue tasks
    {
        {
            let mut executor = ROOT_EXECUTOR.lock();
            enqueue_input_tasks(&mut executor);
        }
        spawn_global(task0);
        spawn_global(task1);
    }
    init::init_pci();
    // Start executing tasks
    Executor::run(&ROOT_EXECUTOR);
    Ok(())
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
fn efi_main(image_handle: EfiHandle, efi_system_table: Pin<&'static os::efi::EfiSystemTable>) {
    os::init::init_basic_runtime(image_handle, efi_system_table);
    println!("rsp on boot: {:#018X}", read_rsp());
    let new_rsp = BootInfo::take().kernel_stack().as_ptr() as usize + os::init::KERNEL_STACK_SIZE;
    unsafe { x86_64::switch_rsp(new_rsp as u64, stack_switched) }
}
