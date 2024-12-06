#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::time::Duration;
use wasabi::error;
use wasabi::executor::sleep;
use wasabi::executor::spawn_global;
use wasabi::executor::start_global_executor;
use wasabi::info;
use wasabi::init::init_allocator;
use wasabi::init::init_basic_runtime;
use wasabi::init::init_display;
use wasabi::init::init_hpet;
use wasabi::init::init_paging;
use wasabi::init::init_pci;
use wasabi::print::hexdump_struct;
use wasabi::print::set_global_vram;
use wasabi::println;
use wasabi::qemu::exit_qemu;
use wasabi::qemu::QemuExitCode;
use wasabi::serial::SerialPort;
use wasabi::uefi::init_vram;
use wasabi::uefi::locate_loaded_image_protocol;
use wasabi::uefi::EfiHandle;
use wasabi::uefi::EfiSystemTable;
use wasabi::warn;
use wasabi::x86::init_exceptions;

#[no_mangle]
fn efi_main(image_handle: EfiHandle, efi_system_table: &EfiSystemTable) {
    println!("Booting WasabiOS...");
    println!("image_handle: {:#018X}", image_handle);
    println!("efi_system_table: {:#p}", efi_system_table);
    let loaded_image_protocol = locate_loaded_image_protocol(image_handle, efi_system_table)
        .expect("Failed to get LoadedImageProtocol");
    println!("image_base: {:#018X}", loaded_image_protocol.image_base);
    println!("image_size: {:#018X}", loaded_image_protocol.image_size);
    info!("info");
    warn!("warn");
    error!("error");
    hexdump_struct(efi_system_table);
    let mut vram = init_vram(efi_system_table).expect("init_vram failed");
    init_display(&mut vram);
    set_global_vram(vram);
    let acpi = efi_system_table.acpi_table().expect("ACPI table not found");

    let memory_map = init_basic_runtime(image_handle, efi_system_table);
    info!("Hello, Non-UEFI world!");
    init_allocator(&memory_map);
    let (_gdt, _idt) = init_exceptions();
    init_paging(&memory_map);
    init_hpet(acpi);
    init_pci(acpi);
    let serial_task = async {
        let sp = SerialPort::default();
        info!("Started to monitor serial port");
        loop {
            if let Some(v) = sp.try_read() {
                let c = char::from_u32(v as u32);
                info!("serial input: {v:#04X} = {c:?}");
            }
            sleep(Duration::from_millis(20)).await;
        }
    };
    spawn_global(serial_task);
    start_global_executor()
}
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    error!("PANIC: {info:?}");
    exit_qemu(QemuExitCode::Fail);
}
