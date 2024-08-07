#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(os::test_runner::test_runner)]
#![feature(sync_unsafe_cell)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use core::pin::Pin;
use noli::bitmap::bitmap_draw_line;
use noli::bitmap::Bitmap;
use os::boot_info::BootInfo;
use os::efi::types::EfiHandle;
use os::error::Result;
use os::executor::spawn_global;
use os::executor::yield_execution;
use os::executor::Executor;
use os::executor::TimeoutFuture;
use os::executor::ROOT_EXECUTOR;
use os::info;
use os::init;
use os::input::enqueue_input_tasks;
use os::println;
use os::x86_64;
use os::x86_64::paging::write_cr3;
use os::x86_64::read_rsp;
use os::x86_64::syscall::init_syscall;

fn paint_wasabi_logo() {
    const SIZE: i64 = 256;
    const COL_SABI: u32 = 0xe33b26;
    const COL_WASABI: u32 = 0x7ec288;

    let mut vram = BootInfo::take().vram();
    let dx = vram.width() / 2 - SIZE;
    let dy = vram.height() / 2 - SIZE;

    // Sabi (Ferris)
    for x in 0..SIZE {
        bitmap_draw_line(
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
        bitmap_draw_line(&mut vram, COL_WASABI, dx, dy, dx + SIZE / 2 + x, dy + SIZE).unwrap();
    }
    for x in 0..SIZE {
        bitmap_draw_line(
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
            bitmap_draw_line(&mut vram, colors[c % 3], x, y, x, y + h)?;
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
            bitmap_draw_line(&mut vram, colors[c % 3], x, y, x, y + h)?;
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
    loop {
        Executor::poll(&ROOT_EXECUTOR);
    }
}

fn main() -> Result<()> {
    info!("Booting WasabiOS...");
    init::init_graphical_terminal();
    paint_wasabi_logo();

    let interrupt_config = init::init_interrupts()?;
    core::mem::forget(interrupt_config);
    init::init_paging()?;
    init::init_timer();
    os::process::init();
    init_syscall();

    // Note: This log message is used by the e2etest to check if the OS is booted
    // so please be careful!
    info!(
        "Welcome to WasabiOS! efi_main = {:#018p}, write_cr3 = {:#018p}",
        efi_main as *const (), write_cr3 as *const ()
    );
    run_tasks()?;
    Ok(())
}

#[no_mangle]
fn stack_switched() -> ! {
    info!("rsp switched to: {:#018X}", read_rsp());
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
