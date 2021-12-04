#![no_std]
#![no_main]
#![feature(asm)]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]

pub mod debug_exit;
pub mod efi;
pub mod error;
pub mod memory_map_holder;
pub mod panic;
pub mod serial;
pub mod test_runner;
pub mod x86;

#[cfg(test)]
#[no_mangle]
fn efi_main(image_handle: efi::EFIHandle, efi_system_table: &efi::EFISystemTable) {
    test_runner::test_prepare(image_handle, efi_system_table);
    test_main();
}
