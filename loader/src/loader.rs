use crate::error::*;
use crate::vram;
use crate::MemoryMapHolder;
use crate::TextArea;
use crate::WasabiBootInfo;
use crate::ALLOCATOR;
use crate::*;
use core::fmt::Write;
use graphics::BitmapImageBuffer;

pub fn main_with_boot_services(
    efi_system_table: &EFISystemTable,
) -> Result<WasabiBootInfo, WasabiError> {
    let mut efi_writer = EFISimpleTextOutputProtocolWriter {
        protocol: efi_system_table.con_out,
    };
    (efi_system_table.con_out.clear_screen)(efi_system_table.con_out).into_result()?;
    writeln!(efi_writer, "Welcome to WasabiOS! ").unwrap();
    writeln!(
        efi_writer,
        "main_with_boot_services started. efi_system_table = {:p}",
        &efi_system_table
    )
    .unwrap();
    let vram = vram::init_vram(efi_system_table).unwrap();

    let mp_services_holder = locate_mp_services_protocol(efi_system_table);
    match mp_services_holder {
        Ok(mp) => {
            writeln!(efi_writer, "MP service found.").unwrap();
            let mut num_proc: usize = 0;
            let mut num_proc_enabled: usize = 0;
            (mp.get_number_of_processors)(mp, &mut num_proc, &mut num_proc_enabled)
                .into_result()?;
            writeln!(
                efi_writer,
                "{}/{} cpus enabled/exists",
                num_proc_enabled, num_proc
            )
            .unwrap();
            let mut info: EFIProcessorInformation = EFIProcessorInformation {
                id: 0,
                status: 0,
                core: 0,
                package: 0,
                thread: 0,
            };
            (mp.get_processor_info)(mp, 0, &mut info).into_result()?;
            writeln!(efi_writer, "{:?}", info).unwrap();
        }
        Err(_) => writeln!(efi_writer, "MP service not found").unwrap(),
    }

    Ok(WasabiBootInfo { vram })
}

pub fn main(info: &WasabiBootInfo, memory_map: &MemoryMapHolder) -> Result<(), WasabiError> {
    println!("hello from serial");

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
        println!("{:?}", e);
    }
    println!("Total memory: {} MiB", total_pages * 4096 / 1024 / 1024);

    textarea.print_string("\nWelcome to Wasabi OS!!!")?;
    Ok(())
}
