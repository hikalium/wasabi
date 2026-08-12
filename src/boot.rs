use crate::cui::Console;
use crate::error;
use crate::executor::sleep;
use crate::executor::spawn_global;
use crate::executor::start_global_executor;
use crate::gui::set_global_vram;
use crate::info;
use crate::init::init_acpi;
use crate::init::init_allocator;
use crate::init::init_apic;
use crate::init::init_basic_runtime;
use crate::init::init_display;
use crate::init::init_hpet;
use crate::init::init_paging;
use crate::init::init_pci;
use crate::input::input_task;
use crate::keyboard::KeyEvent;
use crate::lpss_uart::init_lpss_uart;
use crate::lpss_uart::lpss_uart;
use crate::print::hexdump_struct;
use crate::println;
use crate::ps2kbd::ps2kbd_task;
use crate::serial::SerialPort;
use crate::uefi::init_vram;
use crate::uefi::locate_loaded_image_protocol;
use crate::uefi::EfiHandle;
use crate::uefi::EfiSystemTable;
use crate::warn;
use crate::x86::init_exceptions;
use crate::x86::GdtWrapper;
use crate::x86::Idt;
use core::time::Duration;

/// Initialize every subsystem and spawn the default OS tasks, but do *not*
/// start the executor yet. Pair it with [`run_system`]. Keeping setup and
/// the run loop as two calls lets a caller (notably an integration test)
/// schedule extra tasks in between, before the executor starts draining
/// the queue.
///
/// Returns the `(GdtWrapper, Idt)` produced by `init_exceptions`. **The
/// caller must keep them alive for the whole life of the OS** (bind them
/// to a named local that outlives [`run_system`]). They are registered
/// with the CPU (`lgdt`/`lidt`/`ltr`), and `GdtWrapper` owns the TSS, whose
/// `Drop` deliberately panics ("TSS64 being dropped!"). Dropping the pair
/// therefore both tears down structures the CPU still points at and trips
/// that guard. Moving the pair out is safe: `GdtWrapper`/`Idt` are just
/// `Pin<Box<..>>` handles, so the heap GDT/IDT/TSS the CPU references never
/// move.
pub fn setup_system(
    image_handle: EfiHandle,
    efi_system_table: &EfiSystemTable,
) -> (GdtWrapper, Idt) {
    println!("Booting WasabiOS...");
    println!("image_handle: {:#018X}", image_handle);
    println!("efi_system_table: {:#p}", efi_system_table);
    let loaded_image_protocol =
        locate_loaded_image_protocol(image_handle, efi_system_table)
            .expect("Failed to get LoadedImageProtocol");
    println!("image_base: {:#018X}", loaded_image_protocol.image_base);
    println!("image_size: {:#018X}", loaded_image_protocol.image_size);
    info!("info");
    warn!("warn");
    error!("error");
    hexdump_struct(efi_system_table);
    let mut vram = init_vram(efi_system_table).expect("init_vram failed");
    init_display(&mut vram);
    set_global_vram(vram);
    let acpi = efi_system_table.acpi_table().expect("ACPI table not found");
    init_acpi(acpi);

    let memory_map = init_basic_runtime(image_handle, efi_system_table);
    info!("Hello, Non-UEFI world!");
    init_allocator(&memory_map);
    let descriptor_tables = init_exceptions();
    init_paging(&memory_map);
    init_hpet(acpi);
    init_pci(acpi);
    init_apic().expect("failed to init APIC");

    let serial_task = async {
        let sp = SerialPort::default();
        if let Err(e) = sp.loopback_test() {
            error!("{e:?}");
            return Err("serial: loopback test failed");
        }
        info!("Started to monitor serial port");
        // The serial line is another way to talk to the command shell:
        // feed every received character into a Console of its own.
        // Terminals send CR for the Enter key, which the Console
        // normalizes into a newline that triggers the command.
        let mut console = Console::default();
        loop {
            if let Some(v) = sp.try_read() {
                if let Some(c) = char::from_u32(v as u32) {
                    console.handle_key_down(KeyEvent::Char(c));
                } else {
                    warn!("serial input: not a char: {v:#04X}");
                }
            }
            sleep(Duration::from_millis(20)).await;
        }
    };
    spawn_global(serial_task);
    spawn_global(input_task());
    spawn_global(ps2kbd_task());
    // The LPSS UART carries the AP console that the GSC exposes over
    // Closed Case Debugging, which makes it a way into the shell that
    // needs neither USB nor the network. The base address is the one
    // this board puts the port at; `init_lpss_uart` checks that a
    // DesignWare UART really answers there before adopting it, so a
    // machine (or an emulator) without one just carries on.
    const LPSS_UART_BASE: usize = 0xfe032000; // chromebook boten/bookem
    if init_lpss_uart(LPSS_UART_BASE) {
        info!("LPSS UART at {LPSS_UART_BASE:#x}: console attached");
        let lpss_uart_task = async {
            let uart = lpss_uart().ok_or("lpss uart went away")?;
            // Its own Console, like the serial and remote terminals: the
            // line being edited belongs to whoever is typing it.
            let mut console = Console::default();
            loop {
                // The rx fifo holds 64 bytes, so take everything that is
                // ready before sleeping again.
                while let Some(v) = uart.try_read() {
                    if let Some(c) = char::from_u32(v as u32) {
                        console.handle_key_down(KeyEvent::Char(c));
                    } else {
                        warn!("lpss uart: not a char: {v:#04X}");
                    }
                }
                sleep(Duration::from_millis(20)).await;
            }
        };
        spawn_global(lpss_uart_task);
    } else {
        info!("LPSS UART at {LPSS_UART_BASE:#x}: not present");
    }

    descriptor_tables
}

/// Run the OS main loop: the global async executor. Never returns.
pub fn run_system() -> ! {
    start_global_executor()
}
