use crate::debug_exit;
use crate::efi;
use crate::println;
use crate::serial;
use core::fmt::Write;

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

pub fn test_runner(tests: &[&dyn Testable]) -> ! {
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut writer = serial::SerialConsoleWriter {};
    writeln!(writer, "Running {} tests...", tests.len()).unwrap();
    for test in tests {
        test.run();
    }
    write!(writer, "Done!").unwrap();
    debug_exit::exit_qemu(debug_exit::QemuExitCode::Success)
}

/// This function is called before the tests run and
/// responsible to exit from EFIBootServices and setting up
/// a global allocator for tests.
pub fn test_prepare(image_handle: efi::EFIHandle, efi_system_table: &efi::EFISystemTable) {
    use crate::memory_map_holder::MemoryMapHolder;
    use crate::simple_allocator::ALLOCATOR;

    serial::com_initialize(serial::IO_ADDR_COM2);

    let mut memory_map = MemoryMapHolder::new();
    efi::exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);

    let mut total_pages = 0;
    for e in memory_map.iter() {
        if e.memory_type != efi::EFIMemoryType::CONVENTIONAL_MEMORY {
            continue;
        }
        ALLOCATOR.set_descriptor(e);
        total_pages += e.number_of_pages;
        println!("{:?}", e);
    }
    println!("Total memory: {} MiB", total_pages * 4096 / 1024 / 1024);
}
