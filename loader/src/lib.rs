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
pub mod serial;
pub mod test_runner;
pub mod x86;

use core::fmt::Write;

use core::panic::PanicInfo;
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "panic! {:?}", info).unwrap();
    loop {
        unsafe { asm!("hlt") }
    }
}

#[cfg(test)]
#[no_mangle]
fn efi_main(image_handle: efi::EFIHandle, efi_system_table: &efi::EFISystemTable) {
    test_runner::test_prepare(image_handle, efi_system_table);
    test_main();
}
