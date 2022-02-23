#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use core::fmt::Write;
use os::boot_info::WasabiBootInfo;
use os::efi::*;
use os::error::*;
use os::graphics::*;
use os::memory_map_holder::MemoryMapHolder;
use os::print;
use os::println;
use os::serial;
use os::simple_allocator::ALLOCATOR;
use os::text_area::TextArea;
use os::vram;

pub fn main_with_boot_services(
    efi_system_table: &EFISystemTable,
) -> Result<WasabiBootInfo, WasabiError> {
    let mut efi_writer = EFISimpleTextOutputProtocolWriter {
        protocol: efi_system_table.con_out,
    };
    (efi_system_table.con_out.clear_screen)(efi_system_table.con_out).into_result()?;
    writeln!(efi_writer, "Welcome to WasabiOS!!!! ").unwrap();
    writeln!(
        efi_writer,
        "main_with_boot_services started. efi_system_table = {:p}",
        &efi_system_table
    )
    .unwrap();
    let vram = vram::init_vram(efi_system_table).unwrap();

    let mp_services_holder = locate_mp_services_protocol(efi_system_table);
    match mp_services_holder {
        Ok(mp) => {
            writeln!(efi_writer, "MP service found.").unwrap();
            let mut num_proc: usize = 0;
            let mut num_proc_enabled: usize = 0;
            (mp.get_number_of_processors)(mp, &mut num_proc, &mut num_proc_enabled)
                .into_result()?;
            writeln!(
                efi_writer,
                "{}/{} cpus enabled/exists",
                num_proc_enabled, num_proc
            )
            .unwrap();
            let mut info: EFIProcessorInformation = EFIProcessorInformation {
                id: 0,
                status: 0,
                core: 0,
                package: 0,
                thread: 0,
            };
            (mp.get_processor_info)(mp, 0, &mut info).into_result()?;
            writeln!(efi_writer, "{:?}", info).unwrap();
        }
        Err(_) => writeln!(efi_writer, "MP service not found").unwrap(),
    }

    Ok(WasabiBootInfo { vram })
}

pub fn main(info: &WasabiBootInfo, memory_map: &MemoryMapHolder) -> Result<(), WasabiError> {
    println!("hello from serial");

    let vram = info.vram;
    let textarea = TextArea::new(vram, 8, 16, vram.width() - 16, vram.height() - 32);

    crate::print::GLOBAL_PRINTER.set_text_area(textarea);
    println!("VRAM initialized.");
    println!("Welcome to Wasabi OS!!!");

    let mut total_pages = 0;
    for e in memory_map.iter() {
        if e.memory_type != EFIMemoryType::CONVENTIONAL_MEMORY {
            continue;
        }
        ALLOCATOR.set_descriptor(e);
        total_pages += e.number_of_pages;
        println!("{:?}", e);
    }
    println!("Total memory: {} MiB", total_pages * 4096 / 1024 / 1024);

    Ok(())
}

#[cfg(not(test))]
#[no_mangle]
fn efi_main(image_handle: EFIHandle, efi_system_table: &EFISystemTable) -> ! {
    let info = main_with_boot_services(efi_system_table).unwrap();
    let mut memory_map = MemoryMapHolder::new();
    exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);

    // Initialize serial here since we exited from EFI Boot Services
    serial::com_initialize(serial::IO_ADDR_COM2);
    println!("Exited from EFI Boot Services");

    main(&info, &memory_map).unwrap();

    loop {
        unsafe { core::arch::asm!("pause") }
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
    use os::debug_exit;
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
fn efi_main(image_handle: os::efi::EFIHandle, efi_system_table: &os::efi::EFISystemTable) -> () {
    os::test_runner::test_prepare(image_handle, efi_system_table);
    test_main();
}
