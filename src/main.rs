#![no_std]
#![no_main]
#![feature(asm)]

use core::fmt;
use core::panic::PanicInfo;

pub mod serial;
pub mod x86;

#[repr(C)]
pub struct EFITableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    reserved: u32,
}

pub type EFIHandle = u64;
pub enum EFIStatus {
    SUCCESS = 0,
}

#[repr(C)]
pub struct EFISimpleTextOutputProtocol {
    pub reset: EFIHandle,
    pub output_string:
        extern "win64" fn(this: *const EFISimpleTextOutputProtocol, str: *const u16) -> EFIStatus,
    pub test_string: EFIHandle,
    pub query_mode: EFIHandle,
    pub set_mode: EFIHandle,
    pub set_attribute: EFIHandle,
    pub clear_screen: extern "win64" fn(this: *const EFISimpleTextOutputProtocol) -> EFIStatus,
}

#[repr(C)]
pub struct EFISystemTable<'a> {
    pub header: EFITableHeader,
    pub firmware_vendor: EFIHandle,
    pub firmware_revision: u32,
    pub console_in_handle: EFIHandle,
    pub con_in: EFIHandle,
    pub console_out_handle: EFIHandle,
    pub con_out: &'a EFISimpleTextOutputProtocol,
}

pub struct EFISimpleTextOutputProtocolWriter<'a> {
    pub protocol: &'a EFISimpleTextOutputProtocol,
}

impl EFISimpleTextOutputProtocolWriter<'_> {
    pub fn write_char(&mut self, c: u8) {
        let cbuf: [u16; 2] = [c.into(), 0];
        (self.protocol.output_string)(self.protocol, cbuf.as_ptr());
    }
    pub fn write_str(&mut self, s: &str) {
        for c in s.bytes() {
            if c == b'\n' {
                self.write_char(b'\r');
            }
            self.write_char(c);
        }
    }
}

impl fmt::Write for EFISimpleTextOutputProtocolWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_str(s);
        Ok(())
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { asm!("hlt") }
    }
}

#[no_mangle]
pub extern "win64" fn efi_main(_image_handle: EFIHandle, efi_system_table: EFISystemTable) -> ! {
    use core::fmt::Write;
    serial::com_initialize(serial::IO_ADDR_COM2);
    (efi_system_table.con_out.clear_screen)(efi_system_table.con_out);
    let mut efi_writer = EFISimpleTextOutputProtocolWriter {
        protocol: efi_system_table.con_out,
    };
    writeln!(efi_writer, "Loading wasabiOS...").unwrap();
    writeln!(efi_writer, "{:#p}", &efi_system_table).unwrap();

    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "hello from serial").unwrap();
    loop {
        unsafe { asm!("pause") }
    }
}
