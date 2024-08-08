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
use noli::bitmap::bitmap_draw_line;
use noli::bitmap::bitmap_draw_point;
use noli::bitmap::bitmap_draw_rect;
use noli::bitmap::Bitmap;
use noli::bitmap::BitmapBuffer;
use os::boot_info::BootInfo;
use os::boot_info::File;
use os::cmd;
use os::debug;
use os::efi::fs::EfiFileName;
use os::efi::types::EfiHandle;
use os::error;
use os::error::Error;
use os::error::Result;
use os::executor::spawn_global;
use os::executor::yield_execution;
use os::executor::Executor;
use os::executor::TimeoutFuture;
use os::executor::ROOT_EXECUTOR;
use os::info;
use os::init;
use os::input::InputManager;
use os::print;
use os::println;
use os::serial::SerialPort;
use os::x86_64;
use os::x86_64::read_rsp;
use os::x86_64::syscall::init_syscall;
use sabi::MouseEvent;

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

async fn draw_progress_bar(
    top: i64,
    left: i64,
    width: i64,
    height: i64,
    interval_ms: u64,
) -> Result<()> {
    let mut vram = BootInfo::take().vram();
    let colors = [0xFF0000, 0x00FF00, 0x0000FF];
    let y = top;
    let mut x = left;
    let mut c = 0;
    loop {
        let _ = bitmap_draw_line(&mut vram, colors[c % 3], x, y, x, y + height);
        x += 1;
        if x >= left + width {
            x = left;
            c += 1;
        }
        TimeoutFuture::new_ms(interval_ms).await;
        yield_execution().await;
    }
}

fn run_tasks() -> Result<()> {
    let vram = BootInfo::take().vram();
    let task0 = draw_progress_bar(
        vram.height() / 16 * 14,
        vram.width() / 2,
        vram.width() / 2,
        10,
        10,
    );
    let task1 = draw_progress_bar(
        vram.height() / 16 * 15,
        vram.width() / 2,
        vram.width() / 2,
        10,
        1,
    );
    let serial_task = async {
        let sp = SerialPort::default();
        loop {
            if let Some(c) = sp.try_read() {
                if let Some(c) = char::from_u32(c as u32) {
                    let c = if c == '\r' { '\n' } else { c };
                    InputManager::take().push_input(c);
                }
            }
            TimeoutFuture::new_ms(20).await;
            yield_execution().await;
        }
    };
    let init_task = async {
        info!("running init");
        let boot_info = BootInfo::take();
        let root_files = boot_info.root_files();
        let root_files: alloc::vec::Vec<&File> =
            root_files.iter().filter_map(|e| e.as_ref()).collect();
        let init_txt = EfiFileName::from_str("init.txt")?;
        let init_txt = root_files
            .iter()
            .find(|&e| e.name() == &init_txt)
            .ok_or(Error::Failed("init.txt not found"))?;
        let init_txt = String::from_utf8_lossy(init_txt.data());
        for line in init_txt.trim().split('\n') {
            if let Err(e) = cmd::run(line).await {
                error!("{e:?}");
            };
        }
        Ok(())
    };
    let console_task = async {
        info!("console_task has started");
        let mut s = String::new();
        loop {
            if let Some(c) = InputManager::take().pop_input() {
                if c == '\r' || c == '\n' {
                    if let Err(e) = cmd::run(&s).await {
                        error!("{e:?}");
                    };
                    s.clear();
                }
                match c {
                    '\x7f' | '\x08' => {
                        print!("{0} {0}", 0x08 as char);
                        s.pop();
                    }
                    '\n' => {
                        // Do nothing
                    }
                    _ => {
                        print!("{c}");
                        s.push(c);
                    }
                }
            }
            TimeoutFuture::new_ms(20).await;
            yield_execution().await;
        }
    };
    let mouse_cursor_task = async {
        const CURSOR_SIZE: i64 = 16;
        let mut cursor_bitmap = BitmapBuffer::new(CURSOR_SIZE, CURSOR_SIZE, CURSOR_SIZE);
        for y in 0..CURSOR_SIZE {
            for x in 0..(CURSOR_SIZE - y) {
                if x <= y {
                    bitmap_draw_point(&mut cursor_bitmap, 0x00ff00, x, y)
                        .expect("Failed to paint cursor");
                }
            }
        }
        let mut vram = BootInfo::take().vram();

        noli::bitmap::draw_bmp_clipped(&mut vram, &cursor_bitmap, 100, 100)
            .ok_or(Error::Failed("Failed to draw mouse cursor"))?;

        loop {
            if let Some(MouseEvent {
                position: p,
                button: b,
            }) = InputManager::take().pop_cursor_input_absolute()
            {
                let color = (b.l() as u32) * 0xff0000;
                let color = !color;

                bitmap_draw_rect(&mut vram, color, p.x, p.y, 1, 1)?;
                /*
                crate::graphics::draw_bmp_clipped(&mut vram, &cursor_bitmap, p.x, p.y)
                    .ok_or(Error::Failed("Failed to draw mouse cursor"))?;
                */
            }
            TimeoutFuture::new_ms(15).await;
            yield_execution().await;
        }
    };
    // Enqueue tasks
    spawn_global(task0);
    spawn_global(task1);
    spawn_global(serial_task);
    spawn_global(console_task);
    spawn_global(mouse_cursor_task);
    spawn_global(init_task);
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

    // Note: This log message is used by the e2etest and dbgutil
    // so please do not edit if you are unsure!
    info!("Welcome to WasabiOS!");

    run_tasks()?;
    Ok(())
}

#[no_mangle]
fn stack_switched() -> ! {
    info!("rsp switched to: {:#018X}", read_rsp());
    debug::print_kernel_debug_metadata();
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
