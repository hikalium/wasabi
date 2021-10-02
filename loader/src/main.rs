#![no_std]
#![no_main]
#![feature(asm)]

use crate::efi::*;
use crate::error::*;
use crate::memory_map_holder::*;
use core::fmt::Write;
use core::panic::PanicInfo;

extern crate graphics;

pub mod efi;
pub mod error;
pub mod memory_map_holder;
pub mod serial;
pub mod x86;
pub mod xorshift;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "panic! {:?}", info).unwrap();
    loop {
        unsafe { asm!("hlt") }
    }
}

use graphics::BitmapImageBuffer;

pub fn exit_from_efi_boot_services(
    image_handle: EFIHandle,
    efi_system_table: &EFISystemTable,
    memory_map: &mut memory_map_holder::MemoryMapHolder,
) {
    // Get a memory map and exit boot services
    let status = memory_map_holder::get_memory_map(efi_system_table, memory_map);
    assert_eq!(status, EFIStatus::SUCCESS);
    let status =
        (efi_system_table.boot_services.exit_boot_services)(image_handle, memory_map.map_key);
    assert_eq!(status, EFIStatus::SUCCESS);
}

#[derive(Clone, Copy)]
pub struct VRAMBufferInfo {
    buf: *mut u8,
    width: usize,
    height: usize,
    pixels_per_line: usize,
}

impl BitmapImageBuffer for VRAMBufferInfo {
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

fn init_vram(efi_system_table: &EFISystemTable) -> Result<VRAMBufferInfo, WasabiError> {
    let gp = locate_graphic_protocol(efi_system_table)?;
    Ok(VRAMBufferInfo {
        buf: gp.mode.frame_buffer_base as *mut u8,
        width: gp.mode.info.horizontal_resolution as usize,
        height: gp.mode.info.vertical_resolution as usize,
        pixels_per_line: gp.mode.info.pixels_per_scan_line as usize,
    })
}

pub struct WasabiBootInfo {
    vram: VRAMBufferInfo,
}

fn loader_main_with_boot_services(efi_system_table: &EFISystemTable) -> Result<WasabiBootInfo, ()> {
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

    Ok(WasabiBootInfo { vram })
}

fn loader_main(info: &WasabiBootInfo) -> Result<(), WasabiError> {
    let vram = info.vram;
    let mut rand = xorshift::Xorshift::init();
    let mut y = 0;
    for y in 0..16 {
        for x in 0..16 {
            let col = rand.next().unwrap() as u32;
            let c = (y * 16 + x) as u8 as char;
            graphics::draw_char(&vram, 0xffffff, 0x000000, x * 8 as i64, y * 16 as i64, c).unwrap();
        }
    }
    Ok(())
}

#[no_mangle]
fn efi_main(image_handle: EFIHandle, efi_system_table: &EFISystemTable) -> ! {
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "hello from serial").unwrap();

    let info = loader_main_with_boot_services(efi_system_table).unwrap();
    let mut memory_map = MemoryMapHolder::new();
    exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);
    writeln!(serial_writer, "Exited from EFI Boot Services").unwrap();
    loader_main(&info).unwrap();

    loop {
        unsafe { asm!("pause") }
    }
}
