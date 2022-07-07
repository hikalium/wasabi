extern crate alloc;

use crate::acpi::Acpi;
use crate::boot_info::File;
use crate::efi;
use crate::pci::Pci;
use crate::println;
use crate::util::size_in_pages_from_bytes;
use crate::*;
use alloc::boxed::Box;
use arch::x86_64;
use arch::x86_64::apic::IoApic;
use arch::x86_64::apic::LocalApic;
use arch::x86_64::gdt::GDT;
use arch::x86_64::paging::PML4;
use arch::x86_64::CpuidRequest;
use core::mem::size_of;
use core::slice;
use error::*;
use hpet::Hpet;

pub struct EfiServices {
    image_handle: efi::EfiHandle,
    efi_system_table: &'static mut efi::EfiSystemTable<'static>,
}

impl EfiServices {
    fn new(
        image_handle: efi::EfiHandle,
        efi_system_table: &'static mut efi::EfiSystemTable,
    ) -> Self {
        Self {
            image_handle,
            efi_system_table,
        }
    }
    fn get_loaded_image_protocol(&self) -> &'static mut efi::EfiLoadedImageProtocol<'static> {
        let loaded_image_protocol = self
            .efi_system_table
            .boot_services()
            .handle_loaded_image_protocol(self.image_handle)
            .expect("Failed to get Loaded Image Protocol");
        println!(
            "Got LoadedImageProtocol. Revision: {:#X} system_table: {:#p}",
            loaded_image_protocol.revision, loaded_image_protocol.system_table
        );
        loaded_image_protocol
    }
    pub fn load_all_root_files(&self, root_files: &mut [Option<File>; 32]) -> Result<()> {
        let loaded_image_protocol = self.get_loaded_image_protocol();

        let simple_fs_protocol = self
            .efi_system_table
            .boot_services()
            .handle_simple_file_system_protocol((*loaded_image_protocol).device_handle)
            .expect("Failed to get Simple Filesystem Protocol");
        println!("Got SimpleFileSystemProtocol.",);
        let root_file = simple_fs_protocol.open_volume();
        let root_fs_info = root_file.get_fs_info();
        println!(
            "Got root fs. volume label: {}",
            efi::CStrPtr16::from_ptr(root_fs_info.volume_label.as_ptr())
        );

        // Load all files under root dir
        let mut i = 0;
        while let Some(file_info) = root_file.read_file_info() {
            if file_info.is_dir() {
                println!("DIR : {}", file_info);
                continue;
            }
            println!("FILE: {}", file_info);
            let buf = efi::alloc_byte_slice(self.efi_system_table, file_info.file_size())?;
            println!("allocated buf: {:#p}", buf);
            let file = root_file.open(&file_info.file_name);
            file.read_into_slice(buf)
                .expect("Failed to load file contents");
            if root_files.len() <= i {
                // root_files is full
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
    fn clear_screen(&self) {
        self.efi_system_table
            .con_out()
            .clear_screen()
            .expect("Failed to clear screen");
    }
    fn get_vram_info(&self) -> VRAMBufferInfo {
        vram::init_vram(self.efi_system_table).unwrap()
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
            .get_table_with_guid(&efi::EFI_ACPI_TABLE_GUID)
            .expect("ACPI table not found");

        Acpi::new(rsdp_struct, self)
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
    /// # Safety
    /// this is safe as long as the `size` arg is valid and the data copied does not contain
    /// pointers nor references.
    pub unsafe fn alloc_and_copy<T: 'static>(&self, src: &T, size: usize) -> Result<&'static T> {
        assert!(size_of::<T>() <= size);
        let src = core::slice::from_raw_parts(src as *const T as *const u8, size);
        let dst = self.alloc_boot_data(size)?;
        dst.copy_from_slice(src);
        Ok(&*(dst.as_ptr() as *const T))
    }
}

pub fn init_with_boot_services(
    image_handle: efi::EfiHandle,
    efi_system_table: &'static mut efi::EfiSystemTable,
) {
    serial::com_initialize(serial::IO_ADDR_COM2);
    println!("init_basic_runtime()");
    let efi_services = EfiServices::new(image_handle, efi_system_table);
    efi_services.clear_screen();
    const FILE_NONE: Option<File> = None;
    let mut root_files = [FILE_NONE; 32];
    efi_services
        .load_all_root_files(&mut root_files)
        .expect("Failed to load root files");
    let vram = efi_services.get_vram_info();
    let acpi = efi_services
        .setup_acpi_tables()
        .expect("Failed to setup ACPI tables");
    // Exit from BootServices
    let memory_map = EfiServices::exit_from_boot_services(efi_services);
    let boot_info = BootInfo::new(vram, memory_map, root_files, acpi);
    unsafe {
        BootInfo::set(boot_info);
    }
}

// Common initialization for a normal boot and tests
pub fn init_basic_runtime(
    image_handle: efi::EfiHandle,
    efi_system_table: &'static mut efi::EfiSystemTable,
) {
    init_with_boot_services(image_handle, efi_system_table);
    init_global_allocator();
}

pub fn init_global_allocator() {
    println!("init_global_allocator()");
    crate::allocator::ALLOCATOR.init_with_mmap(BootInfo::take().memory_map());
}

pub fn init_graphical_terminal() {
    println!("init_graphical_terminal()");
    let vram = BootInfo::take().vram();
    let textarea = TextArea::new(vram, 8, 16, vram.width() - 16, vram.height() - 32);
    crate::print::GLOBAL_PRINTER.set_text_area(textarea);
}

pub fn init_paging() -> Result<()> {
    use arch::x86_64::paging::PageAttr;
    use core::cmp::max;
    use efi::EfiMemoryType::*;
    use util::PAGE_SIZE;
    println!("init_paging");
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
    table.create_mappng(0, end_of_mem, 0, PageAttr::ReadWriteKernel)?;
    println!("{:?}", table);
    unsafe {
        crate::arch::x86_64::paging::write_cr3(Box::into_raw(table));
    }
    Ok(())
}

pub fn init_interrupts() {
    println!("init_interrupts()");
    unsafe {
        GDT.load();
        x86_64::write_es(x86_64::KERNEL_DS);
        x86_64::write_cs(x86_64::KERNEL_CS);
        x86_64::write_ss(x86_64::KERNEL_DS);
        x86_64::write_ds(x86_64::KERNEL_DS);
        x86_64::write_fs(x86_64::KERNEL_DS);
        x86_64::write_gs(x86_64::KERNEL_DS);
    }
    x86_64::disable_legacy_pic();
    let bsp_local_apic = LocalApic::new();
    IoApic::init(&bsp_local_apic).expect("Failed to init I/O APIC");
    unsafe {
        x86_64::idt::IDT.init(x86_64::KERNEL_CS);
        x86_64::idt::IDT.load();
    }
}

pub fn detect_fsb_freq() -> Option<u64> {
    let fsb_freq_msr = unsafe { x86_64::read_msr(x86_64::MSR_FSB_FREQ) };
    println!("fsb_freq_msr = {}", fsb_freq_msr);
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
    println!("{:?}", res);
    let freq = res.ecx();
    if freq != 0 {
        freq
    } else if res.ebx() != 0 && res.eax() != 0 {
        let platform_info = unsafe { x86_64::read_msr(x86_64::MSR_PLATFORM_INFO) };
        println!("MSR_PLATFORM_INFO={:#010X}", platform_info);
        ((platform_info as u32 >> 8) & 0xFF) * 1_000_000_000
    } else {
        // Assume that this is QEMU which has ns resolution clock
        1_000_000_000
    }
}

pub fn init_timer() {
    println!("init_timer()");
    let acpi = BootInfo::take().acpi();
    let hpet = unsafe {
        // This is safe since this is the only place to create HPET instance.
        Hpet::new(
            acpi.hpet()
                .base_address()
                .expect("Failed to get HPET base address"),
        )
    };
    println!("{}", hpet.main_counter());
    println!("{}", hpet.main_counter());
    println!("{}", hpet.main_counter());
    println!("{}", hpet.main_counter());
    println!("{}", hpet.main_counter());

    unsafe { core::arch::asm!("int3") }

    println!("I'm back!");

    //unsafe { core::arch::asm!("sti") }
    loop {
        arch::x86_64::hlt();
    }
}

pub fn init_pci() {
    println!("init_pci()");
    let acpi = BootInfo::take().acpi();
    let mcfg = acpi.mcfg();
    let pci = Pci::new(mcfg);
    // This is safe since it is only called once
    unsafe { Pci::set(pci) };
    Pci::take().probe_devices();
    Pci::take().list_devices();
    Pci::take().list_drivers();
}
