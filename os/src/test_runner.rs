use crate::debug;
use crate::info;
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
    info!("Running {} tests...", tests.len());
    for test in tests {
        test.run();
    }
    info!("Done!");
    debug::exit_qemu(debug::QemuExitCode::Success)
}
