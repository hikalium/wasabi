#![no_std]
#![no_main]
#![feature(asm)]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

use crate::efi::*;
use crate::error::*;
use crate::graphics::text_area::*;
use crate::memory_map_holder::*;
use core::fmt::Write;
use core::panic::PanicInfo;

extern crate alloc;

use alloc::vec::Vec;

extern crate graphics;

pub mod debug_exit;
pub mod efi;
pub mod error;
pub mod memory_map_holder;
pub mod serial;
pub mod test_runner;
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

fn loader_main(info: &WasabiBootInfo, memory_map: &MemoryMapHolder) -> Result<(), WasabiError> {
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "hello from serial").unwrap();

    let vram = info.vram;
    let mut textarea = TextArea::new(&vram, 8, 16, vram.width() - 16, vram.height() - 32);
    for _ in 0..200 {
        textarea.print_char('W')?;
    }
    let mut total_pages = 0;
    for e in memory_map.iter() {
        if e.memory_type != EFIMemoryType::CONVENTIONAL_MEMORY {
            continue;
        }
        ALLOCATOR.set_descriptor(e);
        total_pages += e.number_of_pages;
        writeln!(serial_writer, "{:?}", e).unwrap();
    }
    writeln!(
        serial_writer,
        "Total memory: {} MiB",
        total_pages * 4096 / 1024 / 1024
    )
    .unwrap();

    for i in 0..1000 {
        let _a = Vec::<u8>::with_capacity(2);
        writeln!(serial_writer, "{}", i).unwrap();
    }
    let _a = Vec::<u8>::with_capacity(8192);

    textarea.print_string("\nWelcome to Wasabi OS!!!")?;
    Ok(())
}

#[cfg(not(test))]
#[no_mangle]
fn efi_main(image_handle: EFIHandle, efi_system_table: &EFISystemTable) -> ! {
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut serial_writer = serial::SerialConsoleWriter {};
    writeln!(serial_writer, "hello from serial").unwrap();

    let info = loader_main_with_boot_services(efi_system_table).unwrap();
    let mut memory_map = MemoryMapHolder::new();
    exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);
    writeln!(serial_writer, "Exited from EFI Boot Services").unwrap();
    loader_main(&info, &memory_map).unwrap();

    loop {
        unsafe { asm!("pause") }
    }
}

use alloc::alloc::GlobalAlloc;
use alloc::alloc::Layout;
use core::cell::Cell;

struct SimpleAllocator {
    free_info: Cell<Option<&'static FreeInfo>>,
}

#[global_allocator]
static ALLOCATOR: SimpleAllocator = SimpleAllocator {
    free_info: Cell::new(None),
};

/*
phys: ....1.........23....456...
virt: .......123456.............
*/

unsafe impl Sync for SimpleAllocator {}

unsafe impl GlobalAlloc for SimpleAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        serial::com_initialize(serial::IO_ADDR_COM2);
        let mut serial_writer = serial::SerialConsoleWriter {};
        writeln!(serial_writer, "alloc: {:?}", layout).unwrap();
        let free_info = (*self.free_info.as_ptr()).expect("free_info is None");
        let pages_needed = (layout.size() + 4095) / 4096;
        if pages_needed > *free_info.num_of_pages.as_ptr() {
            core::ptr::null_mut::<u8>()
        } else {
            /*
            .......######################.........
                   ^
            ***
                                      ***
                   <-number_of_pages->
            */
            *free_info.num_of_pages.as_ptr() -= pages_needed;
            if *free_info.num_of_pages.as_ptr() == 0 {
                // Releases the free info since it is empty
                *self.free_info.as_ptr() = *free_info.next_free_info.as_ptr();
            }
            (free_info as *const FreeInfo as *mut u8).add(*free_info.num_of_pages.as_ptr() * 4096)
        }
    }
    /*
        ....######.....
        ....#####@.....
        ....######.....
            XXXXX
                 X

    */
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        serial::com_initialize(serial::IO_ADDR_COM2);
        let mut serial_writer = serial::SerialConsoleWriter {};
        writeln!(serial_writer, "dealloc: {:?} ptr {:?}", layout, ptr).unwrap();
        let pages_being_freed = (layout.size() + 4095) / 4096;
        let info = &mut *(ptr as *mut FreeInfo);
        *info.next_free_info.as_ptr() = *self.free_info.as_ptr();
        *info.num_of_pages.as_ptr() = pages_being_freed;
        *self.free_info.as_ptr() = Some(info);
    }
}

struct FreeInfo {
    next_free_info: Cell<Option<&'static FreeInfo>>,
    num_of_pages: Cell<usize>,
}
// Assume that FreeInfo is smaller than 4KiB

/*
............#############....##...
            ^                ^
            |<---------------|
root------------------------>|
*/

impl SimpleAllocator {
    fn set_descriptor(&self, desc: &EFIMemoryDescriptor) {
        // Create linked list here???
        serial::com_initialize(serial::IO_ADDR_COM2);
        let mut serial_writer = serial::SerialConsoleWriter {};
        writeln!(serial_writer, "set_descriptor: {:?}", desc).unwrap();
        unsafe {
            let info = &mut *(desc.physical_start as *mut FreeInfo);
            *info.next_free_info.as_ptr() = None;
            *info.num_of_pages.as_ptr() = desc.number_of_pages as usize;
            *self.free_info.as_ptr() = Some(info);
        }
    }
}

#[cfg(test)]
#[start]
pub extern "win64" fn _start() -> ! {
    test_main();
    loop {}
}

pub trait Testable {
    fn run(&self);
}

impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        serial::com_initialize(serial::IO_ADDR_COM2);
        let mut writer = serial::SerialConsoleWriter {};
        write!(writer, "{}...\t", core::any::type_name::<T>()).unwrap();
        self();
        writeln!(writer, "[PASS]").unwrap();
    }
}

#[cfg(test)]
fn test_runner(tests: &[&dyn Testable]) -> ! {
    serial::com_initialize(serial::IO_ADDR_COM2);
    let mut writer = serial::SerialConsoleWriter {};
    writeln!(writer, "Running {} tests...", tests.len()).unwrap();
    for test in tests {
        test.run();
    }
    write!(writer, "Done!").unwrap();
    debug_exit::exit_qemu(debug_exit::QemuExitCode::Success)
}

#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}

#[cfg(test)]
#[no_mangle]
fn efi_main(image_handle: efi::EFIHandle, efi_system_table: &efi::EFISystemTable) -> () {
    test_runner::test_prepare(image_handle, efi_system_table);
    test_main();
}
