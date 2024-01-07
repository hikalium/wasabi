#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![feature(associated_type_defaults)]
#![test_runner(crate::test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![allow(incomplete_features)]
#![feature(adt_const_params)]
#![feature(const_option)]
#![feature(new_uninit)]
#![feature(sync_unsafe_cell)]
#![deny(clippy::wildcard_imports)]
#![deny(clippy::enum_glob_use)]
#![feature(associated_type_bounds)]
#![feature(inherent_associated_types)]
#![feature(asm_const)]
#![feature(alloc_layout_extra)]
#![feature(bigint_helper_methods)]
#![feature(array_chunks)]
#![feature(iter_array_chunks)]
#![feature(iter_advance_by)]
#![feature(linked_list_cursors)]
#![feature(offset_of)]

mod acpi;
mod allocator;
mod ax88179;
mod bitset;
pub mod boot_info;
mod command;
pub mod debug_exit;
pub mod efi;
pub mod elf;
pub mod error;
pub mod executor;
pub mod graphics;
mod hpet;
pub mod init;
pub mod input;
pub mod loader;
mod memory;
mod memory_map_holder;
mod mutex;
mod net;
mod panic;
mod pci;
pub mod print;
mod ps2;
mod rtl8139;
mod serial;
mod syscall;
pub mod test_runner;
mod text_area;
mod usb;
mod usb_hid_keyboard;
mod usb_hid_tablet;
mod util;
mod volatile;
mod vram;
pub mod x86_64;
mod xhci;

#[cfg(test)]
#[no_mangle]
// For unit tests except that are in main.rs
fn efi_main(
    image_handle: efi::types::EfiHandle,
    efi_system_table: core::pin::Pin<&'static efi::EfiSystemTable>,
) {
    crate::init::init_basic_runtime(image_handle, efi_system_table);
    test_main();
}
