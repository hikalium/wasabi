use crate::efi;
use crate::*;
use core::mem::size_of;
use core::ptr::null_mut;
use error::*;

struct EfiServices {
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
        let mut loaded_image_protocol: *mut efi::EfiLoadedImageProtocol =
            null_mut::<efi::EfiLoadedImageProtocol>();
        unsafe {
            let status = (self
                .efi_system_table
                .boot_services
                .handle_protocol
                .handle_loaded_image_protocol)(
                self.image_handle,
                &efi::EFI_LOADED_IMAGE_PROTOCOL_GUID,
                &mut loaded_image_protocol,
            );
            assert_eq!(status, efi::EfiStatus::SUCCESS);
            println!(
                "Got LoadedImageProtocol. Revision: {:#X} system_table: {:#p}",
                (*loaded_image_protocol).revision,
                (*loaded_image_protocol).system_table
            );
        }
        unsafe { &mut *loaded_image_protocol }
    }
    pub fn load_all_root_files(&self) -> Result<(), WasabiError> {
        let loaded_image_protocol = self.get_loaded_image_protocol();

        let mut simple_file_system_protocol: *mut efi::EfiSimpleFileSystemProtocol =
            null_mut::<efi::EfiSimpleFileSystemProtocol>();
        unsafe {
            let status = (self
                .efi_system_table
                .boot_services
                .handle_protocol
                .handle_simple_file_system_protocol)(
                (*loaded_image_protocol).device_handle,
                &efi::EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
                &mut simple_file_system_protocol,
            );
            assert_eq!(status, efi::EfiStatus::SUCCESS);
            println!(
                "Got SimpleFileSystemProtocol. revision: {:#X}",
                (*simple_file_system_protocol).revision
            );
        }

        let mut root_file: *mut efi::EfiFileProtocol = null_mut::<efi::EfiFileProtocol>();
        unsafe {
            let status = ((*simple_file_system_protocol).open_volume)(
                simple_file_system_protocol,
                &mut root_file,
            );
            assert_eq!(status, efi::EfiStatus::SUCCESS);
            println!(
                "Got FileProtocol of the root file. revision: {:#X}",
                (*root_file).revision
            );
        }

        let mut root_fs_info: efi::EfiFileSystemInfo = efi::EfiFileSystemInfo::default();
        let mut root_fs_info_size: usize = size_of::<efi::EfiFileSystemInfo>();
        unsafe {
            let status = ((*root_file).get_info)(
                root_file,
                &efi::EFI_FILE_SYSTEM_INFO_GUID,
                &mut root_fs_info_size,
                &mut root_fs_info,
            );
            assert_eq!(status, efi::EfiStatus::SUCCESS);
            println!(
                "Got root fs. volume label: {}",
                efi::CStrPtr16::from_ptr(root_fs_info.volume_label.as_ptr())
            );
        }

        // List all files under root dir
        loop {
            let mut file_info: efi::EfiFileInfo = efi::EfiFileInfo::default();
            let mut file_info_size;
            unsafe {
                file_info_size = size_of::<efi::EfiFileInfo>();
                let status = ((*root_file).read)(root_file, &mut file_info_size, &mut file_info);
                assert_eq!(status, efi::EfiStatus::SUCCESS);
                if file_info_size == 0 {
                    break;
                }
                if file_info.is_dir() {
                    continue;
                }
                println!("FILE: {}", file_info);
                let num_of_pages = util::size_in_pages_from_bytes(file_info.size as usize);
                let buf = efi::alloc_pages(self.efi_system_table, num_of_pages)?;
                println!("allocated buf: {:#p}", buf);
            }
        }
        Ok(())
    }
    fn clear_screen(&self) {
        (self.efi_system_table.con_out.clear_screen)(self.efi_system_table.con_out)
            .into_result()
            .unwrap();
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
}

pub fn init_basic_runtime(
    image_handle: efi::EfiHandle,
    efi_system_table: &'static mut efi::EfiSystemTable,
) {
    serial::com_initialize(serial::IO_ADDR_COM2);
    crate::println!("init_basic_runtime()");
    let efi_services = EfiServices::new(image_handle, efi_system_table);
    efi_services.clear_screen();
    efi_services
        .load_all_root_files()
        .expect("Failed to load root files");
    let vram = efi_services.get_vram_info();

    let memory_map = EfiServices::exit_from_boot_services(efi_services);
    let boot_info = BootInfo::new(vram, memory_map);
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
