#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![feature(associated_type_defaults)]
#![test_runner(crate::test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![feature(const_maybe_uninit_zeroed)]
#![allow(incomplete_features)]
#![feature(adt_const_params)]
#![feature(const_option)]
#![feature(const_result_drop)]
#![feature(new_uninit)]
#![feature(saturating_int_impl)]

pub mod acpi;
pub mod allocator;
pub mod arch;
pub mod boot_info;
pub mod debug_exit;
pub mod efi;
pub mod elf;
pub mod error;
pub mod graphics;
pub mod hpet;
pub mod init;
pub mod memory_map_holder;
pub mod network;
pub mod panic;
pub mod pci;
pub mod print;
pub mod rtl8139;
pub mod serial;
pub mod test_runner;
pub mod text_area;
pub mod util;
pub mod vram;
pub mod xhci;

#[cfg(test)]
#[no_mangle]
// For unit tests except that are in main.rs
fn efi_main(image_handle: efi::EfiHandle, efi_system_table: &'static mut efi::EfiSystemTable) {
    crate::init::init_basic_runtime(image_handle, efi_system_table);
    test_main();
}

// Structs
pub use boot_info::BootInfo;
pub use memory_map_holder::MemoryMapHolder;
pub use text_area::TextArea;
pub use vram::VRAMBufferInfo;

// Trait impls
use crate::graphics::BitmapImageBuffer;
