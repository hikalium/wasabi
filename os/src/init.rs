use crate::boot_info::File;
use crate::efi;
use crate::util::size_in_pages_from_bytes;
use crate::x86::*;
use crate::*;
use acpi::Acpi;
use apic::LocalApic;
use core::mem::size_of;
use core::slice;
use core::str;
use error::*;

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

pub fn init_basic_runtime(
    image_handle: efi::EfiHandle,
    efi_system_table: &'static mut efi::EfiSystemTable,
) {
    serial::com_initialize(serial::IO_ADDR_COM2);
    crate::println!("init_basic_runtime()");
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

pub fn init_global_allocator() {
    crate::println!("init_global_allocator()");
    crate::allocator::ALLOCATOR.init_with_mmap(BootInfo::take().memory_map());
}

pub fn init_graphical_terminal() {
    crate::println!("init_graphical_terminal()");
    let vram = BootInfo::take().vram();
    let textarea = TextArea::new(vram, 8, 16, vram.width() - 16, vram.height() - 32);
    crate::print::GLOBAL_PRINTER.set_text_area(textarea);
}

pub fn init_interrupts() {
    crate::println!("init_interrupts()");
    x86::disable_legacy_pic();
    let _bsp_local_apic = LocalApic::new();
    let c = x86::read_cpuid(CpuidRequest { eax: 0, ecx: 0 });
    crate::println!("cpuid(0, 0) = {:?}", c);
    let slice = unsafe { slice::from_raw_parts(&c as *const CpuidResponse as *const u8, 12) };
    let vendor = str::from_utf8(slice).expect("failed to parse utf8 str");
    crate::println!("cpuid(0, 0).vendor = {}", vendor);
}

pub fn init_pci() {
    crate::println!("init_pci()");
    let acpi = BootInfo::take().acpi();
    let mcfg = acpi.mcfg();
    println!("{:?}", mcfg);
    for i in 0..mcfg.num_of_entries() {
        let e = mcfg.entry(i).expect("Out of range");
        println!("{}", e);
    }
}
