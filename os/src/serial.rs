use crate::x86_64::busy_loop_hint;
use crate::x86_64::read_io_port_u8;
use crate::x86_64::write_io_port_u8;
use core::convert::TryInto;
use core::fmt;

// https://wiki.osdev.org/Serial_Ports
#[repr(u16)]
#[derive(Clone, Copy)]
pub enum SerialPortIndex {
    Com1 = 0x3f8,
    Com2 = 0x2f8,
}

pub struct SerialPort {
    index: SerialPortIndex,
}
impl SerialPort {
    pub fn new(index: SerialPortIndex) -> Self {
        Self { index }
    }
    fn base(&self) -> u16 {
        self.index as u16
    }
    pub fn init(&mut self) {
        write_io_port_u8(self.base() + 1, 0x00); // Disable all interrupts
        write_io_port_u8(self.base() + 3, 0x80); // Enable DLAB (set baud rate divisor)
        const BAUD_DIVISOR: u16 = 0x0001; // baud rate = (115200 / BAUD_DIVISOR)
        write_io_port_u8(self.base(), (BAUD_DIVISOR & 0xff).try_into().unwrap());
        write_io_port_u8(self.base() + 1, (BAUD_DIVISOR >> 8).try_into().unwrap());
        write_io_port_u8(self.base() + 3, 0x03); // 8 bits, no parity, one stop bit
        write_io_port_u8(self.base() + 2, 0xC7); // Enable FIFO, clear them, with 14-byte threshold
        write_io_port_u8(self.base() + 4, 0x0B); // IRQs enabled, RTS/DSR set
    }
    pub fn send_char(&self, c: char) {
        while (read_io_port_u8(self.base() + 5) & 0x20) == 0 {
            busy_loop_hint();
        }
        write_io_port_u8(self.base(), c as u8)
    }
    pub fn try_read(&self) -> Option<u8> {
        if read_io_port_u8(self.base() + 5) & 0x01 == 0 {
            None
        } else {
            Some(read_io_port_u8(self.base()))
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
        Self::new(SerialPortIndex::Com2)
    }
}
