#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![feature(associated_type_defaults)]
#![test_runner(crate::test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]

pub mod boot_info;
pub mod debug_exit;
pub mod efi;
pub mod error;
pub mod graphics;
pub mod memory_map_holder;
pub mod panic;
pub mod print;
pub mod serial;
pub mod test_runner;
pub mod text_area;
pub mod vram;
pub mod x86;

#[path = "simple_allocator.rs"]
pub mod allocator;

#[cfg(test)]
#[no_mangle]
fn efi_main(image_handle: efi::EFIHandle, efi_system_table: &efi::EFISystemTable) {
    test_runner::run_tests(image_handle, efi_system_table, &test_main);
}
