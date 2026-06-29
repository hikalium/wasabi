#![no_std]
#![no_main]

use core::panic::PanicInfo;
use wasabi::boot::run_system;
use wasabi::boot::setup_system;
use wasabi::qemu::exit_qemu;
use wasabi::qemu::QemuExitCode;
use wasabi::uefi::EfiHandle;
use wasabi::uefi::EfiSystemTable;

#[no_mangle]
fn efi_main(image_handle: EfiHandle, efi_system_table: &EfiSystemTable) {
    // Hold the GDT/IDT for the life of the OS: run_system() never returns,
    // so this binding lives forever. Dropping them would tear down
    // CPU-registered tables and trip the TSS's panicking Drop guard.
    let _descriptor_tables = setup_system(image_handle, efi_system_table);
    run_system()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    wasabi::print::panic_print(format_args!("[ERROR] PANIC: {info:?}\n"));
    exit_qemu(QemuExitCode::Fail);
}
