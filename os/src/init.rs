use crate::*;

pub fn init_basic_runtime(
    image_handle: efi::EFIHandle,
    efi_system_table: &mut efi::EFISystemTable,
) {
    crate::println!("init_basic_runtime()");
    (efi_system_table.con_out.clear_screen)(efi_system_table.con_out)
        .into_result()
        .unwrap();
    let vram = vram::init_vram(efi_system_table).unwrap();

    // Initialize serial here since we exited from EFI Boot Services
    serial::com_initialize(serial::IO_ADDR_COM2);

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
