#![no_std]
#![no_main]
#![feature(asm)]

use core::fmt;
use core::panic::PanicInfo;

extern crate graphics;

pub mod serial;
pub mod x86;

#[repr(C)]
pub struct EFI_GUID {
    data0: u32,
    data1: u16,
    data2: u16,
    data3: [u8; 8],
}

pub const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: EFI_GUID = EFI_GUID {
    data0: 0x9042a9de,
    data1: 0x23dc,
    data2: 0x4a38,
    data3: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};

pub type EFIVoid = u8;

#[repr(C)]
pub struct EFITableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    reserved: u32,
}

pub type EFIHandle = u64;

#[derive(Debug, PartialEq)]
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
#[derive(Debug)]
pub struct EFIGraphicsOutputProtocolPixelInfo {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: u32,
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub reserved_mask: u32,
    pub pixels_per_scan_line: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct EFIGraphicsOutputProtocolMode<'a> {
    pub max_mode: u32,
    pub mode: u32,
    pub info: &'a EFIGraphicsOutputProtocolPixelInfo,
    pub size_of_info: u64,
    pub frame_buffer_base: usize,
    pub frame_buffer_size: usize,
}

#[repr(C)]
#[derive(Debug)]
pub struct EFIGraphicsOutputProtocol<'a> {
    reserved: [u64; 3],
    pub mode: &'a EFIGraphicsOutputProtocolMode<'a>,
}

pub struct EFIBootServicesTable {
    pub header: EFITableHeader,
    _reserved: [u64; 37],
    pub locate_protocol: extern "win64" fn(
        protocol: *const EFI_GUID,
        registration: *const EFIVoid,
        interface: *mut *mut EFIGraphicsOutputProtocol,
    ) -> EFIStatus,
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
    pub standard_error_handle: EFIHandle,
    pub std_err: EFIHandle,
    pub runtime_services: EFIHandle,
    pub boot_services: &'a EFIBootServicesTable,
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
fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write;
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "panic! {:?}", info).unwrap();
    loop {
        unsafe { asm!("hlt") }
    }
}

fn locate_graphic_protocol<'a>(
    efi_system_table: &'a EFISystemTable,
) -> Result<&'a EFIGraphicsOutputProtocol<'a>, ()> {
    use core::ptr::null_mut;
    let mut graphic_output_protocol: *mut EFIGraphicsOutputProtocol =
        null_mut::<EFIGraphicsOutputProtocol>();
    let status = (efi_system_table.boot_services.locate_protocol)(
        &EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
        null_mut::<EFIVoid>(),
        &mut graphic_output_protocol,
    );
    if status != EFIStatus::SUCCESS {
        return Err(());
    }
    Ok(unsafe { &*graphic_output_protocol })
}

#[no_mangle]
fn efi_main(_image_handle: EFIHandle, efi_system_table: EFISystemTable) -> ! {
    use core::fmt::Write;
    (efi_system_table.con_out.clear_screen)(efi_system_table.con_out);
    let mut efi_writer = EFISimpleTextOutputProtocolWriter {
        protocol: efi_system_table.con_out,
    };
    writeln!(efi_writer, "Loading wasabiOS...").unwrap();
    writeln!(efi_writer, "{:#p}", &efi_system_table).unwrap();

    let gp = locate_graphic_protocol(&efi_system_table).unwrap();
    writeln!(efi_writer, "{:?}", gp.mode.frame_buffer_base).unwrap();
    writeln!(efi_writer, "{:?}", gp.mode.frame_buffer_size).unwrap();
    writeln!(efi_writer, "{:?}", gp).unwrap();
    writeln!(efi_writer, "{:X}", gp.mode.info.red_mask).unwrap();

    let base_addr = gp.mode.frame_buffer_base as *mut u8;
    for y in 0..gp.mode.info.vertical_resolution / 2 {
        for x in 0..gp.mode.info.horizontal_resolution / 2 {
            for i in 1..2 {
                // BGR-
                unsafe {
                    *base_addr
                        .add((i + (y * gp.mode.info.pixels_per_scan_line + x) * 4) as usize) = 0xff;
                }
            }
        }
    }

    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "hello from serial").unwrap();
    loop {
        unsafe { asm!("pause") }
    }
}
