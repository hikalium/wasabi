#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;
extern crate graphics;

use core::arch::asm;
use core::fmt::Write;
use loader::efi::*;
use loader::memory_map_holder::*;
use loader::println;
use loader::serial;

#[cfg(not(test))]
#[no_mangle]
fn efi_main(image_handle: EFIHandle, efi_system_table: &EFISystemTable) -> ! {
    let info = loader::loader::main_with_boot_services(efi_system_table).unwrap();
    let mut memory_map = MemoryMapHolder::new();
    exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);

    // Initialize serial here since we exited from EFI Boot Services
    serial::com_initialize(serial::IO_ADDR_COM2);
    println!("Exited from EFI Boot Services");

    loader::loader::main(&info, &memory_map).unwrap();

    loop {
        unsafe { asm!("pause") }
    }
}

#[cfg(test)]
#[start]
pub extern "win64" fn _start() -> ! {
    test_main();
    loop {}
}

pub trait Testable {
    fn run(&self);
}

impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        serial::com_initialize(serial::IO_ADDR_COM2);
        let mut writer = serial::SerialConsoleWriter {};
        write!(writer, "{}...\t", core::any::type_name::<T>()).unwrap();
        self();
        writeln!(writer, "[PASS]").unwrap();
    }
}

#[cfg(test)]
fn test_runner(tests: &[&dyn Testable]) -> ! {
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut writer = serial::SerialConsoleWriter {};
    writeln!(writer, "Running {} tests...", tests.len()).unwrap();
    for test in tests {
        test.run();
    }
    write!(writer, "Done!").unwrap();
    debug_exit::exit_qemu(debug_exit::QemuExitCode::Success)
}

#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}

#[cfg(test)]
#[no_mangle]
fn efi_main(image_handle: efi::EFIHandle, efi_system_table: &efi::EFISystemTable) -> () {
    test_runner::test_prepare(image_handle, efi_system_table);
    test_main();
}
