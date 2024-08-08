use crate::println;
use crate::serial::SerialPort;
use core::fmt::Write;
use core::panic::PanicInfo;

#[cfg(test)]
use crate::debug;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut serial_writer = SerialPort::default();
    writeln!(serial_writer, "[PANIC] {:?}", info).unwrap();
    println!("[PANIC] {:?}", info);
    #[cfg(not(test))]
    crate::x86_64::rest_in_peace();
    #[cfg(test)]
    debug::exit_qemu(debug::QemuExitCode::Fail);
}
