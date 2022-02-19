use crate::serial;
use core::fmt::Write;
use core::panic::PanicInfo;

#[cfg(test)]
use crate::debug_exit;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "panic! {:?}", info).unwrap();
    #[cfg(not(test))]
    loop {
        use core::arch::asm;
        unsafe { asm!("hlt") }
    }
    #[cfg(test)]
    debug_exit::exit_qemu(debug_exit::QemuExitCode::Fail);
}
