#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(os::test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use os::arch::x86_64::read_rsp;
use os::boot_info::BootInfo;
use os::error::*;
use os::graphics;
use os::graphics::BitmapImageBuffer;
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
        graphics::draw_line(
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
        graphics::draw_line(&mut vram, COL_WASABI, dx, dy, dx + SIZE / 2 + x, dy + SIZE).unwrap();
    }
    for x in 0..SIZE {
        graphics::draw_line(
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

fn main() -> Result<()> {
    use core::fmt::Write;
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

    let boot_info = BootInfo::take();
    let root_files = boot_info.root_files();
    let root_files: alloc::vec::Vec<&os::boot_info::File> =
        root_files.iter().filter_map(|e| e.as_ref()).collect();
    os::println!("Number of root files: {}", root_files.len());
    for (i, f) in root_files.iter().enumerate() {
        os::println!("root_files[{}]: {}", i, f.name());
    }
    for i in 1..=8 {
        let base_addr = serial::IO_ADDR_COM[i - 1];
        serial::com_initialize(base_addr);
        let mut w = serial::SerialConsoleWriter::new(base_addr);
        writeln!(w, "COM{}!", i).expect("Failed to write");
        os::println!("Printed to COM{}", i);
    }
    unsafe {
        core::arch::asm!("ud2");
    }
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
