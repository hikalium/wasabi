use crate::x86_64::read_io_port_u8;
use crate::x86_64::write_io_port_u8;
use core::arch::asm;
use core::convert::TryInto;
use core::fmt;

// https://wiki.osdev.org/Serial_Ports
pub const IO_ADDR_COM1: u16 = 0x3f8;
pub const IO_ADDR_COM2: u16 = 0x2f8;
pub const IO_ADDR_COM3: u16 = 0x3e8;
pub const IO_ADDR_COM4: u16 = 0x2e8;
pub const IO_ADDR_COM5: u16 = 0x5f8;
pub const IO_ADDR_COM6: u16 = 0x4f8;
pub const IO_ADDR_COM7: u16 = 0x5e8;
pub const IO_ADDR_COM8: u16 = 0x4e8;
pub const IO_ADDR_COM: [u16; 8] = [
    IO_ADDR_COM1,
    IO_ADDR_COM2,
    IO_ADDR_COM3,
    IO_ADDR_COM4,
    IO_ADDR_COM5,
    IO_ADDR_COM6,
    IO_ADDR_COM7,
    IO_ADDR_COM8,
];

pub fn com_initialize(base_io_addr: u16) {
    write_io_port_u8(base_io_addr + 1, 0x00); // Disable all interrupts
    write_io_port_u8(base_io_addr + 3, 0x80); // Enable DLAB (set baud rate divisor)
    const BAUD_DIVISOR: u16 = 0x0001; // baud rate = (115200 / BAUD_DIVISOR)
    write_io_port_u8(base_io_addr, (BAUD_DIVISOR & 0xff).try_into().unwrap());
    write_io_port_u8(base_io_addr + 1, (BAUD_DIVISOR >> 8).try_into().unwrap());
    write_io_port_u8(base_io_addr + 3, 0x03); // 8 bits, no parity, one stop bit
    write_io_port_u8(base_io_addr + 2, 0xC7); // Enable FIFO, clear them, with 14-byte threshold
    write_io_port_u8(base_io_addr + 4, 0x0B); // IRQs enabled, RTS/DSR set
}

pub struct SerialPort {
    base_io_addr: u16,
}
impl SerialPort {
    pub fn new(base_io_addr: u16) -> Self {
        Self { base_io_addr }
    }
    pub fn send_char(&self, c: char) {
        while (read_io_port_u8(self.base_io_addr + 5) & 0x20) == 0 {
            unsafe { asm!("pause") }
        }
        write_io_port_u8(self.base_io_addr, c as u8)
    }
    pub fn try_read(&self) -> Option<u8> {
        if read_io_port_u8(self.base_io_addr + 5) & 0x01 == 0 {
            None
        } else {
            Some(read_io_port_u8(self.base_io_addr))
        }
    }

    pub fn send_str(&self, s: &str) {
        let mut sc = s.chars();
        let slen = s.chars().count();
        for _ in 0..slen {
            self.send_char(sc.next().unwrap());
        }
    }
}
impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let serial = Self::default();
        serial.send_str(s);
        Ok(())
    }
}
impl Default for SerialPort {
    fn default() -> Self {
        Self::new(IO_ADDR_COM2)
    }
}
