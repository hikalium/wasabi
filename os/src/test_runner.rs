use crate::debug_exit;
use crate::efi;
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
/// responsible to exit from EfiBootServices and setting up
/// a global allocator for tests.
pub fn run_tests(
    image_handle: efi::EfiHandle,
    efi_system_table: &'static mut efi::EfiSystemTable,
    test_main: &dyn Fn(),
) {
    use crate::init::*;
    init_basic_runtime(image_handle, efi_system_table);
    init_global_allocator();
    test_main();
}
