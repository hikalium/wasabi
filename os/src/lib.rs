#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![feature(associated_type_defaults)]
#![test_runner(crate::test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]

pub mod acpi;
pub mod arch;
pub mod boot_info;
pub mod debug_exit;
pub mod efi;
pub mod error;
pub mod graphics;
pub mod init;
pub mod memory_map_holder;
pub mod panic;
pub mod pci;
pub mod print;
pub mod serial;
pub mod test_runner;
pub mod text_area;
pub mod util;
pub mod vram;

// #[path = "simple_allocator.rs"]
#[path = "first_fit_allocator.rs"]
pub mod allocator;

#[cfg(test)]
#[no_mangle]
fn efi_main(image_handle: efi::EfiHandle, efi_system_table: &'static mut efi::EfiSystemTable) {
    test_runner::run_tests(image_handle, efi_system_table, &test_main);
}

// Structs
pub use boot_info::BootInfo;
pub use memory_map_holder::MemoryMapHolder;
pub use text_area::TextArea;
pub use vram::VRAMBufferInfo;

// Trait impls
use crate::graphics::BitmapImageBuffer;
