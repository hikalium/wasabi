#![no_std]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner::test_runner)]
#![reexport_test_harness_main = "run_unit_tests"]
#![no_main]
pub mod allocator;
pub mod graphics;
pub mod qemu;
pub mod result;
pub mod serial;
pub mod uefi;
pub mod x86;

#[cfg(test)]
pub mod test_runner;

#[cfg(test)]
#[no_mangle]
fn efi_main(image_handle: uefi::EfiHandle, efi_system_table: &uefi::EfiSystemTable) {
    let mut memory_map = uefi::MemoryMapHolder::new();
    uefi::exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);

    allocator::ALLOCATOR.init_with_mmap(&memory_map);
    run_unit_tests()
}
