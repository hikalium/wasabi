use crate::println;
use crate::serial;
use core::fmt::Write;
use core::panic::PanicInfo;

#[cfg(test)]
use crate::debug_exit;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut serial_writer = serial::SerialConsoleWriter::default();
    writeln!(serial_writer, "panic! {:?}", info).unwrap();
    println!("panic! {:?}", info);
    #[cfg(not(test))]
    loop {
        use core::arch::asm;
        unsafe { asm!("hlt") }
    }
    #[cfg(test)]
    debug_exit::exit_qemu(debug_exit::QemuExitCode::Fail);
}
