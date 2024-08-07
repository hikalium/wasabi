extern crate alloc;

use crate::acpi::Acpi;
use crate::boot_info::BootInfo;
use crate::boot_info::File;
use crate::efi;
use crate::error;
use crate::hpet;
use crate::memory_map_holder;
use crate::pci::Pci;
use crate::serial::SerialPort;
use crate::serial::SerialPortIndex;
use crate::util;
use crate::util::size_in_pages_from_bytes;
use crate::vram;
use crate::vram::VRAMBufferInfo;
use crate::x86_64;
use crate::x86_64::apic::IoApic;
use crate::x86_64::block_interrupts;
use crate::x86_64::gdt::Gdt;
use crate::x86_64::idt::Idt;
use crate::x86_64::idt::TaskStateSegment64;
use crate::x86_64::paging::write_cr3;
use crate::x86_64::paging::PageAttr;
use crate::x86_64::paging::PML4;
use crate::x86_64::CpuidRequest;
use alloc::boxed::Box;
use core::cmp::max;
use core::fmt::Write;
use core::pin::Pin;
use core::slice;
use efi::types::EfiHandle;
use efi::EfiMemoryType::CONVENTIONAL_MEMORY;
use efi::EfiMemoryType::LOADER_CODE;
use efi::EfiMemoryType::LOADER_DATA;
use error::Result;
use hpet::Hpet;
use noli::bitmap::Bitmap;
use noli::text_area;
use noli::text_area::TextArea;
use util::PAGE_SIZE;

pub const KERNEL_STACK_SIZE: usize = 1024 * 1024;

pub struct EfiServices {
    image_handle: EfiHandle,
    efi_system_table: Pin<&'static efi::EfiSystemTable>,
}

impl EfiServices {
    fn new(image_handle: EfiHandle, efi_system_table: Pin<&'static efi::EfiSystemTable>) -> Self {
        Self {
            image_handle,
            efi_system_table,
        }
    }
    fn get_loaded_image_protocol(&self) -> Pin<&efi::EfiLoadedImageProtocol> {
        let boot_services = self.efi_system_table.boot_services();
        boot_services
            .handle_loaded_image_protocol(self.image_handle)
            .expect("Failed to get Loaded Image Protocol")
    }
    pub fn load_all_root_files(&self, root_files: &mut [Option<File>; 32]) -> Result<()> {
        let loaded_image_protocol = self.get_loaded_image_protocol();
        let boot_services = self.efi_system_table.boot_services();
        let simple_fs_protocol = boot_services
            .handle_simple_file_system_protocol(loaded_image_protocol.device_handle)
            .expect("Failed to get Simple Filesystem Protocol");
        let root_file = simple_fs_protocol
            .open_volume()
            .expect("Failed to get root_file");
        // Load all files under root dir
        let mut i = 0;
        while let Some(file_info) = root_file.as_ref().read_file_info() {
            if file_info.is_dir() {
                continue;
            }
            let buf = efi::alloc_byte_slice(self.efi_system_table, file_info.file_size())?;
            let file = root_file.open(&file_info.file_name);
            file.read_into_slice(buf)
                .expect("Failed to load file contents");
            if root_files.len() <= i {
                panic!("No more space left for root_files");
            }
            unsafe {
                root_files[i] = Some(File::from_raw(
                    file_info.file_name(),
                    buf.as_mut_ptr(),
                    file_info.file_size(),
                )?);
            }
            i += 1;
        }
        Ok(())
    }
    fn get_vram_info(&self) -> Result<VRAMBufferInfo> {
        let mut vram = vram::init_vram(self.efi_system_table).unwrap();
        let w = vram.width();
        let h = vram.height();
        noli::bitmap::bitmap_draw_rect(&mut vram, 0x101010, 0, 0, w, h)?;
        Ok(vram)
    }
    fn exit_from_boot_services(efi_services: Self) -> memory_map_holder::MemoryMapHolder {
        let mut memory_map = memory_map_holder::MemoryMapHolder::new();
        efi::exit_from_efi_boot_services(
            efi_services.image_handle,
            efi_services.efi_system_table,
            &mut memory_map,
        );
        memory_map
    }
    fn setup_acpi_tables(&self) -> Result<Acpi> {
        let rsdp_struct = self
            .efi_system_table
            .get_table_with_guid(&efi::constants::EFI_ACPI_TABLE_GUID)
            .expect("ACPI table not found");

        Acpi::new(rsdp_struct)
    }
    pub fn alloc_boot_data(&self, size: usize) -> Result<&'static mut [u8]> {
        // This is safe since it constructs a slice with the same size of allocated buf.
        Ok(unsafe {
            slice::from_raw_parts_mut(
                efi::alloc_pages(self.efi_system_table, size_in_pages_from_bytes(size))?,
                size,
            )
        })
    }
}

pub fn init_with_boot_services(
    image_handle: EfiHandle,
    efi_system_table: Pin<&'static efi::EfiSystemTable>,
) {
    {
        let mut serial = SerialPort::new(SerialPortIndex::Com1);
        serial.init();
        writeln!(serial, "WasabiOS COM1 Initialized").expect("Failed to print out to COM1");
    }
    {
        let mut serial = SerialPort::new(SerialPortIndex::Com2);
        serial.init();
        writeln!(serial, "WasabiOS COM2 Initialized").expect("Failed to print out to COM2");
    }
    let kernel_stack = efi::alloc_pages(
        efi_system_table,
        size_in_pages_from_bytes(KERNEL_STACK_SIZE),
    )
    .expect("Not enough space for the kernel stack");
    if kernel_stack.is_null() {
        panic!("Failed to allocate kernel stack");
    }
    let kernel_stack = unsafe { slice::from_raw_parts_mut(kernel_stack, KERNEL_STACK_SIZE) };
    let efi_services = EfiServices::new(image_handle, efi_system_table);
    const FILE_NONE: Option<File> = None;
    let mut root_files = [FILE_NONE; 32];
    efi_services
        .load_all_root_files(&mut root_files)
        .expect("Failed to load root files");
    let vram = efi_services.get_vram_info().expect("Failed to init vram");
    let acpi = efi_services
        .setup_acpi_tables()
        .expect("Failed to setup ACPI tables");
    // Exit from BootServices
    let memory_map = EfiServices::exit_from_boot_services(efi_services);
    let boot_info = BootInfo::new(vram, memory_map, root_files, acpi, kernel_stack);
    unsafe {
        BootInfo::set(boot_info);
    }
}

// Common initialization for a normal boot and tests
pub fn init_basic_runtime(
    image_handle: EfiHandle,
    efi_system_table: Pin<&'static efi::EfiSystemTable>,
) {
    init_with_boot_services(image_handle, efi_system_table);
    init_global_allocator();
}

pub fn init_global_allocator() {
    crate::allocator::ALLOCATOR.init_with_mmap(BootInfo::take().memory_map());
}

pub fn init_graphical_terminal() {
    let vram = BootInfo::take().vram();
    let mut textarea = TextArea::new(
        vram,
        0,
        vram.height() / 4 * 3,
        vram.width(),
        vram.height() / 4,
    );
    textarea.set_mode(text_area::TextAreaMode::Ring);
    crate::print::GLOBAL_PRINTER.set_text_area(textarea);
}

pub fn init_paging() -> Result<()> {
    let mut table = PML4::new();
    let memory_map = BootInfo::take().memory_map();
    let mut end_of_mem = 0x1_0000_0000u64;
    for e in memory_map.iter() {
        match e.memory_type {
            CONVENTIONAL_MEMORY | LOADER_CODE | LOADER_DATA => {
                end_of_mem = max(
                    end_of_mem,
                    e.physical_start + e.number_of_pages * (PAGE_SIZE as u64),
                );
            }
            _ => (),
        }
    }
    table.create_mapping(0, end_of_mem, 0, PageAttr::ReadWriteKernel)?;
    unsafe {
        write_cr3(Box::into_raw(table));
    }
    Ok(())
}

#[allow(dead_code)]
pub struct InterruptConfiguration {
    tss64: Pin<Box<TaskStateSegment64>>,
    gdt: Pin<Box<Gdt>>,
    idt: Pin<Box<Idt>>,
}

pub fn init_interrupts() -> Result<InterruptConfiguration> {
    block_interrupts();
    let tss64 = TaskStateSegment64::new()?;
    let gdt = Gdt::new(&tss64)?;
    unsafe {
        x86_64::write_cs(x86_64::KERNEL_CS);
        x86_64::write_ss(x86_64::KERNEL_DS);
        x86_64::write_es(x86_64::KERNEL_DS);
        x86_64::write_ds(x86_64::KERNEL_DS);
        x86_64::write_fs(x86_64::KERNEL_DS);
        x86_64::write_gs(x86_64::KERNEL_DS);
    }
    x86_64::disable_legacy_pic();
    let bsp_local_apic = BootInfo::take().bsp_local_apic();
    IoApic::init(bsp_local_apic).expect("Failed to init I/O APIC");
    let idt = Idt::new(x86_64::KERNEL_CS)?;
    Ok(InterruptConfiguration { tss64, gdt, idt })
}

pub fn detect_fsb_freq() -> Option<u64> {
    let fsb_freq_msr = unsafe { x86_64::read_msr(x86_64::MSR_FSB_FREQ) };
    let fsb_khz = match fsb_freq_msr & 0b111 {
        0b101 => 100_000,
        0b001 => 133_333,
        0b011 => 166_666,
        0b010 => 200_000,
        0b000 => 266_666,
        0b100 => 333_333,
        0b110 => 400_000,
        _ => return None,
    };
    Some(fsb_khz)
}

pub fn detect_core_clock_freq() -> u32 {
    let res = x86_64::read_cpuid(CpuidRequest { eax: 0x15, ecx: 0 });
    let freq = res.ecx();
    if freq != 0 {
        freq
    } else if res.ebx() != 0 && res.eax() != 0 {
        let platform_info = unsafe { x86_64::read_msr(x86_64::MSR_PLATFORM_INFO) };
        ((platform_info as u32 >> 8) & 0xFF) * 1_000_000_000
    } else {
        // Assume that this is QEMU which has ns resolution clock
        1_000_000_000
    }
}

pub fn init_timer() {
    let acpi = BootInfo::take().acpi();
    unsafe {
        // This is safe since this is the only place to create HPET instance.
        Hpet::set(Hpet::new(
            acpi.hpet()
                .base_address()
                .expect("Failed to get HPET base address"),
        ));
    }
}

pub fn init_pci() {
    let acpi = BootInfo::take().acpi();
    let mcfg = acpi.mcfg();
    let pci = Pci::new(mcfg);
    // This is safe since it is only called once
    unsafe { Pci::set(pci) };
    Pci::take()
        .probe_devices()
        .expect("Failed to probe devices");
}
