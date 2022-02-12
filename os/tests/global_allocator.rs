#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]

use os::efi;
use os::test_runner;

#[cfg(test)]
#[no_mangle]
fn efi_main(image_handle: efi::EFIHandle, efi_system_table: &efi::EFISystemTable) {
    test_runner::test_prepare(image_handle, efi_system_table);
    test_main();
}
