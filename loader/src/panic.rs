use core::panic::PanicInfo;

use crate::serial;
use core::fmt::Write;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "panic! {:?}", info).unwrap();
    loop {
        unsafe { asm!("hlt") }
    }
}
