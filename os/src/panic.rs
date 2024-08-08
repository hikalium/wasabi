use crate::println;
use crate::serial::SerialPort;
use crate::x86_64::read_rbp;
use crate::x86_64::read_rsp;
use core::fmt::Write;
use core::panic::PanicInfo;

#[cfg(test)]
use crate::debug;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut serial_writer = SerialPort::default();
    let rsp = read_rsp();
    writeln!(serial_writer, "[PANIC] RSP = {rsp:#018X}").unwrap();
    let rbp = read_rbp();
    writeln!(serial_writer, "[PANIC] RBP = {rbp:#018X}").unwrap();
    println!("[PANIC] {:?}", info);
    #[cfg(not(test))]
    crate::x86_64::rest_in_peace();
    #[cfg(test)]
    debug::exit_qemu(debug::QemuExitCode::Fail);
}
