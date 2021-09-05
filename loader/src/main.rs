#![no_std]
#![no_main]
#![feature(asm)]

use core::fmt;
use core::panic::PanicInfo;

extern crate graphics;

pub mod serial;
pub mod x86;
pub mod xorshift;

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

pub const EFI_MP_SERVICES_PROTOCOL_GUID: EFI_GUID = EFI_GUID {
    data0: 0x3fdda605,
    data1: 0xa76e,
    data2: 0x4f46,
    data3: [0xad, 0x29, 0x12, 0xf4, 0x53, 0x1b, 0x3d, 0x08],
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

#[repr(C)]
#[derive(Debug)]
pub struct EFIProcessorInformation {
    pub id: u64,
    pub status: u32,
    pub package: u32,
    pub core: u32,
    pub thread: u32,
}

#[repr(C)]
pub struct EFIMPServicesProtocol {
    pub get_number_of_processors: extern "win64" fn(
        this: *const EFIMPServicesProtocol,
        num_of_proc: &mut usize,
        num_of_proc_enabled: &mut usize,
    ) -> EFIStatus,
    pub get_processor_info: extern "win64" fn(
        this: *const EFIMPServicesProtocol,
        proc_num: usize,
        info: &mut EFIProcessorInformation,
    ) -> EFIStatus,
}

pub struct EFIBootServicesTable {
    pub header: EFITableHeader,
    _reserved: [u64; 37],
    pub locate_protocol: extern "win64" fn(
        protocol: *const EFI_GUID,
        registration: *const EFIVoid,
        interface: *mut *mut EFIVoid,
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

fn locate_mp_services_protocol<'a>(
    efi_system_table: &'a EFISystemTable,
) -> Result<&'a EFIMPServicesProtocol, ()> {
    use core::ptr::null_mut;
    let mut protocol: *mut EFIMPServicesProtocol = null_mut::<EFIMPServicesProtocol>();
    let status = (efi_system_table.boot_services.locate_protocol)(
        &EFI_MP_SERVICES_PROTOCOL_GUID,
        null_mut::<EFIVoid>(),
        &mut protocol as *mut *mut EFIMPServicesProtocol as *mut *mut EFIVoid,
    );
    if status != EFIStatus::SUCCESS {
        return Err(());
    }
    Ok(unsafe { &*protocol })
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
        &mut graphic_output_protocol as *mut *mut EFIGraphicsOutputProtocol as *mut *mut EFIVoid,
    );
    if status != EFIStatus::SUCCESS {
        return Err(());
    }
    Ok(unsafe { &*graphic_output_protocol })
}

use graphics::BitmapImageBuffer;

pub struct VRAMBuffer {
    buf: *mut u8,
    width: usize,
    height: usize,
    pixels_per_line: usize,
}

impl BitmapImageBuffer for VRAMBuffer {
    fn bytes_per_pixel(&self) -> i64 {
        4
    }
    fn pixels_per_line(&self) -> i64 {
        self.pixels_per_line as i64
    }
    fn width(&self) -> i64 {
        self.width as i64
    }
    fn height(&self) -> i64 {
        self.height as i64
    }
    fn buf(&self) -> *mut u8 {
        self.buf
    }
    unsafe fn pixel_at(&self, x: i64, y: i64) -> *mut u8 {
        self.buf()
            .add(((y * self.pixels_per_line() + x) * self.bytes_per_pixel()) as usize)
    }
    fn flush(&self) {
        // Do nothing
    }
    fn is_in_x_range(&self, px: i64) -> bool {
        0 <= px && px < self.width as i64
    }
    fn is_in_y_range(&self, py: i64) -> bool {
        0 <= py && py < self.height as i64
    }
}

fn init_vram(efi_system_table: &EFISystemTable) -> Result<VRAMBuffer, ()> {
    let gp = locate_graphic_protocol(efi_system_table)?;
    Ok(VRAMBuffer {
        buf: gp.mode.frame_buffer_base as *mut u8,
        width: gp.mode.info.horizontal_resolution as usize,
        height: gp.mode.info.vertical_resolution as usize,
        pixels_per_line: gp.mode.info.pixels_per_scan_line as usize,
    })
}

fn loader_main(efi_system_table: &EFISystemTable) -> Result<(), ()> {
    use core::fmt::Write;
    (efi_system_table.con_out.clear_screen)(efi_system_table.con_out);
    let mut efi_writer = EFISimpleTextOutputProtocolWriter {
        protocol: efi_system_table.con_out,
    };
    writeln!(efi_writer, "Loading wasabiOS...").unwrap();
    writeln!(efi_writer, "{:#p}", &efi_system_table).unwrap();

    let vram = init_vram(efi_system_table).unwrap();

    let mp_services_holder = locate_mp_services_protocol(efi_system_table);
    match mp_services_holder {
        Ok(mp) => {
            writeln!(efi_writer, "MP service found").unwrap();
            let mut num_proc: usize = 0;
            let mut num_proc_enabled: usize = 0;
            let status = (mp.get_number_of_processors)(mp, &mut num_proc, &mut num_proc_enabled);
            writeln!(
                efi_writer,
                "status = {:?}, {}/{} cpu(s) enabled",
                status, num_proc_enabled, num_proc
            )
            .unwrap();
            let mut info: EFIProcessorInformation = EFIProcessorInformation {
                id: 0,
                status: 0,
                core: 0,
                package: 0,
                thread: 0,
            };
            let status = (mp.get_processor_info)(mp, 0, &mut info);
            writeln!(efi_writer, "status = {:?}, info = {:?}", status, info).unwrap();
        }
        Err(_) => writeln!(efi_writer, "MP service not found").unwrap(),
    }

    let mut rand = xorshift::Xorshift::init();
    for _ in 0..100000 {
        let xsize = (rand.next().unwrap() as i64).rem_euclid(vram.width() / 4 - 10) + 10;
        let ysize = (rand.next().unwrap() as i64).rem_euclid(vram.width() / 4 - 10) + 10;
        graphics::draw_rect(
            &vram,
            rand.next().unwrap() as u32,
            (rand.next().unwrap() as i64).rem_euclid(vram.width() - xsize),
            (rand.next().unwrap() as i64).rem_euclid(vram.height() - ysize),
            xsize,
            ysize,
        )
        .unwrap();
    }

    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "hello from serial").unwrap();
    Ok(())
}

#[no_mangle]
fn efi_main(_image_handle: EFIHandle, efi_system_table: &EFISystemTable) -> ! {
    loader_main(efi_system_table).unwrap();
    loop {
        unsafe { asm!("pause") }
    }
}
