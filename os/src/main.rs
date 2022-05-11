#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(os::test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use os::boot_info::BootInfo;
use os::error::*;
use os::graphics;
use os::graphics::BitmapImageBuffer;

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

pub fn main() -> Result<(), WasabiError> {
    os::println!("Booting Wasabi OS!!!");
    paint_wasabi_logo();
    let boot_info = BootInfo::take();
    let root_files = boot_info.root_files();
    let root_files: alloc::vec::Vec<&os::boot_info::File> =
        root_files.iter().filter_map(|e| e.as_ref()).collect();
    os::println!("Number of root files: {}", root_files.len());
    for (i, f) in root_files.iter().enumerate() {
        os::println!("root_files[{}]: {}", i, f.name());
        os::print::hexdump(f.data());
    }

    Ok(())
}

#[cfg(not(test))]
#[no_mangle]
fn efi_main(
    image_handle: os::efi::EfiHandle,
    efi_system_table: &'static mut os::efi::EfiSystemTable,
) -> ! {
    use os::init::*;
    init_basic_runtime(image_handle, efi_system_table);
    init_graphical_terminal();
    init_global_allocator();
    main().unwrap();
    loop {
        unsafe { core::arch::asm!("pause") }
    }
}

#[cfg(test)]
#[no_mangle]
fn efi_main(
    image_handle: os::efi::EfiHandle,
    efi_system_table: &'static mut os::efi::EfiSystemTable,
) {
    os::test_runner::run_tests(image_handle, efi_system_table, &test_main);
}
