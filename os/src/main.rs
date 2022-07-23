#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(os::test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use os::arch::x86_64::read_rsp;
use os::boot_info::BootInfo;
use os::elf::Elf;
use os::error::*;
use os::graphics::draw_line;
use os::graphics::BitmapImageBuffer;
use os::print;
use os::println;

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

fn delay() {
    for _ in 0..10000 {
        os::arch::x86_64::busy_loop_hint();
    }
}
fn pseudo_multitask() -> Result<()> {
    let mut vram = BootInfo::take().vram();
    let colors = [0xFF0000, 0x00FF00, 0x0000FF];
    let h = 10;
    // Task 0
    let y1 = vram.height() / 3;
    let mut x1 = 0;
    let mut c1 = 0;
    // Task 1
    let y2 = vram.height() / 3 * 2;
    let mut x2 = 0;
    let mut c2 = 0;
    for t in 0.. {
        if t % 4 == 0 {
            // Do some work for task 0 (when t == 0 under mod 4)
            draw_line(&mut vram, colors[c1 % 3], x1, y1, x1, y1 + h)?;
            x1 += 1;
            if x1 >= vram.width() {
                x1 = 0;
                c1 += 1;
            }
        } else {
            // Do some work for task 1 (when t == 1, 2, 3 under mod 4)
            draw_line(&mut vram, colors[c2 % 3], x2, y2, x2, y2 + h)?;
            x2 += 1;
            if x2 >= vram.width() {
                x2 = 0;
                c2 += 1;
            }
        }
        delay();
        print!(".");
    }
    Ok(())
}

fn main() -> Result<()> {
    use os::*;
    init::init_graphical_terminal();
    os::println!("Booting Wasabi OS!!!");
    println!("Initial rsp = {:#018X}", arch::x86_64::read_rsp());
    paint_wasabi_logo();

    unsafe { core::arch::asm!("cli") }
    init::init_interrupts();
    init::init_paging()?;
    init::init_timer();
    init::init_pci();

    println!("Wasabi OS booted.");

    let boot_info = BootInfo::take();
    let root_files = boot_info.root_files();
    let root_files: alloc::vec::Vec<&os::boot_info::File> =
        root_files.iter().filter_map(|e| e.as_ref()).collect();
    println!("Number of root files: {}", root_files.len());
    for (i, f) in root_files.iter().enumerate() {
        println!("root_files[{}]: {}", i, f.name());
    }
    let elf = root_files.iter().find(|&e| e.has_name("hello"));
    if let Some(elf) = elf {
        let elf = Elf::new(elf);
        println!("Executable found: {:?} ", elf);
        elf.exec().expect("Failed to parse ELF");
    }

    pseudo_multitask()?;
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

    os::arch::x86_64::rest_in_peace()
}

#[no_mangle]
fn efi_main(
    image_handle: os::efi::EfiHandle,
    efi_system_table: &'static mut os::efi::EfiSystemTable,
) {
    os::init::init_basic_runtime(image_handle, efi_system_table);
    println!("rsp on boot: {:#018X}", read_rsp());
    let new_rsp = BootInfo::take().kernel_stack().as_ptr() as usize + os::init::KERNEL_STACK_SIZE;
    unsafe { os::arch::x86_64::switch_rsp(new_rsp as u64, stack_switched) }
}
