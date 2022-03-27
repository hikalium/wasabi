#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(os::test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use os::error::*;

pub fn main() -> Result<(), WasabiError> {
    os::println!("Booting Wasabi OS!!!");
    Ok(())
}

#[test_case]
fn malloc_iterate_free_and_alloc() {}

#[cfg(not(test))]
#[no_mangle]
fn efi_main(image_handle: os::efi::EFIHandle, efi_system_table: &mut os::efi::EFISystemTable) -> ! {
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
fn efi_main(image_handle: os::efi::EFIHandle, efi_system_table: &mut os::efi::EFISystemTable) {
    os::test_runner::run_tests(image_handle, efi_system_table, &test_main);
}
