use crate::x86;
use core::convert::TryInto;
use core::fmt;

// const IO_ADDR_COM1: u16 = 0x3f8;
pub const IO_ADDR_COM2: u16 = 0x2f8;
pub fn com_initialize(base_io_addr: u16) {
    x86::write_io_port(base_io_addr + 1, 0x00); // Disable all interrupts
    x86::write_io_port(base_io_addr + 3, 0x80); // Enable DLAB (set baud rate divisor)
    const BAUD_DIVISOR: u16 = 0x0001; // baud rate = (115200 / BAUD_DIVISOR)
    x86::write_io_port(base_io_addr, (BAUD_DIVISOR & 0xff).try_into().unwrap());
    x86::write_io_port(base_io_addr + 1, (BAUD_DIVISOR >> 8).try_into().unwrap());
    x86::write_io_port(base_io_addr + 3, 0x03); // 8 bits, no parity, one stop bit
    x86::write_io_port(base_io_addr + 2, 0xC7); // Enable FIFO, clear them, with 14-byte threshold
    x86::write_io_port(base_io_addr + 4, 0x0B); // IRQs enabled, RTS/DSR set
}

pub fn com_send_char(base_io_addr: u16, c: char) {
    while (x86::read_io_port(base_io_addr + 5) & 0x20) == 0 {
        unsafe { asm!("pause") }
    }
    x86::write_io_port(base_io_addr, c as u8)
}

pub fn com_send_str(base_io_addr: u16, s: &str) {
    let mut sc = s.chars();
    let slen = s.chars().count();
    for _ in 0..slen {
        com_send_char(base_io_addr, sc.next().unwrap());
    }
}

pub struct SerialConsoleWriter {}

impl SerialConsoleWriter {}

impl fmt::Write for SerialConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        com_send_str(IO_ADDR_COM2, s);
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
        ($($arg:tt)*) => ($crate::serial::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
        () => ($crate::print!("\n"));
            ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut writer = crate::serial::SerialConsoleWriter {};
    fmt::write(&mut writer, args).unwrap();
}
