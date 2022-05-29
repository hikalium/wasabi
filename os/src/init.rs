use crate::boot_info::File;
use crate::efi;
use crate::*;
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
                .boot_services()
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
    pub fn load_all_root_files(
        &self,
        root_files: &mut [Option<File>; 32],
    ) -> Result<(), WasabiError> {
        let loaded_image_protocol = self.get_loaded_image_protocol();

        let mut simple_fs_protocol: *mut efi::EfiSimpleFileSystemProtocol =
            null_mut::<efi::EfiSimpleFileSystemProtocol>();
        unsafe {
            let status = (self
                .efi_system_table
                .boot_services()
                .handle_protocol
                .handle_simple_file_system_protocol)(
                (*loaded_image_protocol).device_handle,
                &efi::EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
                &mut simple_fs_protocol,
            );
            assert_eq!(status, efi::EfiStatus::SUCCESS);
            println!("Got SimpleFileSystemProtocol.",);
        }
        let simple_fs_protocol = unsafe { &*simple_fs_protocol };
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

    let memory_map = EfiServices::exit_from_boot_services(efi_services);
    let boot_info = BootInfo::new(vram, memory_map, root_files);
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
