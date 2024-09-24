#![no_std]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner::test_runner)]
#![reexport_test_harness_main = "run_unit_tests"]
#![no_main]
pub mod allocator;
pub mod graphics;
pub mod qemu;
pub mod result;
pub mod uefi;
pub mod x86;

#[cfg(test)]
pub mod test_runner;

#[cfg(test)]
#[no_mangle]
pub fn efi_main() {
    run_unit_tests()
}
