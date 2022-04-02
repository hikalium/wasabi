use crate::*;
use core::mem::size_of;
use error::*;

pub fn load_all_root_files(
    image_handle: efi::EFIHandle,
    efi_system_table: &mut efi::EFISystemTable,
) -> Result<(), WasabiError> {
    let mut loaded_image_protocol: *mut efi::EFILoadedImageProtocol =
        0 as *mut efi::EFILoadedImageProtocol;
    unsafe {
        let status = (efi_system_table
            .boot_services
            .handle_protocol
            .handle_loaded_image_protocol)(
            image_handle,
            &efi::EFI_LOADED_IMAGE_PROTOCOL_GUID,
            &mut loaded_image_protocol,
        );
        assert_eq!(status, efi::EFIStatus::SUCCESS);
        println!(
            "Got LoadedImageProtocol. Revision: {:#X} system_table: {:#p}",
            (*loaded_image_protocol).revision,
            (*loaded_image_protocol).system_table
        );
    }

    let mut simple_file_system_protocol: *mut efi::EFISimpleFileSystemProtocol =
        0 as *mut efi::EFISimpleFileSystemProtocol;
    unsafe {
        let status = (efi_system_table
            .boot_services
            .handle_protocol
            .handle_simple_file_system_protocol)(
            (*loaded_image_protocol).device_handle,
            &efi::EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
            &mut simple_file_system_protocol,
        );
        assert_eq!(status, efi::EFIStatus::SUCCESS);
        println!(
            "Got SimpleFileSystemProtocol. revision: {:#X}",
            (*simple_file_system_protocol).revision
        );
    }

    let mut root_file: *mut efi::EFIFileProtocol = 0 as *mut efi::EFIFileProtocol;
    unsafe {
        let status = ((*simple_file_system_protocol).open_volume)(
            simple_file_system_protocol,
            &mut root_file,
        );
        assert_eq!(status, efi::EFIStatus::SUCCESS);
        println!(
            "Got FileProtocol of the root file. revision: {:#X}",
            (*root_file).revision
        );
    }

    let mut root_fs_info: efi::EFIFileSystemInfo = efi::EFIFileSystemInfo::default();
    let mut root_fs_info_size: efi::EFINativeUInt = size_of::<efi::EFIFileSystemInfo>();
    unsafe {
        let status = ((*root_file).get_info)(
            root_file,
            &efi::EFI_FILE_SYSTEM_INFO_GUID,
            &mut root_fs_info_size,
            &mut root_fs_info,
        );
        assert_eq!(status, efi::EFIStatus::SUCCESS);
        println!(
            "Got root fs. volume label: {}",
            efi::CStrPtr16::from_ptr(root_fs_info.volume_label.as_ptr())
        );
    }

    // List all files under root dir
    loop {
        let mut file_info: efi::EFIFileInfo = efi::EFIFileInfo::default();
        let mut file_info_size;
        unsafe {
            file_info_size = size_of::<efi::EFIFileInfo>();
            let status = ((*root_file).read)(root_file, &mut file_info_size, &mut file_info);
            assert_eq!(status, efi::EFIStatus::SUCCESS);
            if file_info_size == 0 {
                break;
            }
            println!("FILE: {:?}", file_info);
        }
    }
    Ok(())
}

pub fn init_basic_runtime(
    image_handle: efi::EFIHandle,
    efi_system_table: &mut efi::EFISystemTable,
) {
    serial::com_initialize(serial::IO_ADDR_COM2);
    crate::println!("init_basic_runtime()");
    (efi_system_table.con_out.clear_screen)(efi_system_table.con_out)
        .into_result()
        .unwrap();
    load_all_root_files(image_handle, efi_system_table).expect("Failed to load root files");
    let vram = vram::init_vram(efi_system_table).unwrap();

    let mut memory_map = memory_map_holder::MemoryMapHolder::new();
    efi::exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);

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
