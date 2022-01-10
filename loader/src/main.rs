#![no_std]
#![no_main]
#![feature(asm)]
#![feature(alloc_error_handler)]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;
extern crate graphics;

pub mod debug_exit;
pub mod efi;
pub mod error;
pub mod loader;
pub mod memory_map_holder;
pub mod panic;
pub mod serial;
pub mod test_runner;
pub mod vram;
pub mod x86;
pub mod xorshift;

use crate::efi::*;
use crate::graphics::text_area::*;
use crate::memory_map_holder::*;
use core::fmt::Write;

pub struct WasabiBootInfo {
    vram: vram::VRAMBufferInfo,
}

#[cfg(not(test))]
#[no_mangle]
fn efi_main(image_handle: EFIHandle, efi_system_table: &EFISystemTable) -> ! {
    let info = loader::main_with_boot_services(efi_system_table).unwrap();
    let mut memory_map = MemoryMapHolder::new();
    exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);

    // Initialize serial here since we exited from EFI Boot Services
    serial::com_initialize(serial::IO_ADDR_COM2);
    println!("Exited from EFI Boot Services");

    loader::main(&info, &memory_map).unwrap();

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
