use crate::debug_exit;
use crate::serial;
use core::any::type_name;
use core::fmt::Write;
use serial::SerialPort;
use serial::SerialPortIndex;

pub trait Testable {
    fn run(&self);
}

impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        let mut writer = SerialPort::new(SerialPortIndex::Com2);
        writer.init();
        let mut writer = SerialPort::default();
        writeln!(writer, "[RUNNING] >>> {}", type_name::<T>()).unwrap();
        self();
        writeln!(writer, "[PASS   ] <<< {}", type_name::<T>()).unwrap();
    }
}

pub fn test_runner(tests: &[&dyn Testable]) -> ! {
    let mut writer = SerialPort::new(SerialPortIndex::Com2);
    writer.init();
    writeln!(writer, "Running {} tests...", tests.len()).unwrap();
    for test in tests {
        test.run();
    }
    write!(writer, "Done!").unwrap();
    debug_exit::exit_qemu(debug_exit::QemuExitCode::Success)
}
