use crate::info;
use crate::x86_64;

/// This function prints the addr of itself for debug purpose.
#[no_mangle]
pub fn print_kernel_debug_metadata() {
    info!(
        "DEBUG_METADATA: print_kernel_debug_metadata = {:#018p}",
        print_kernel_debug_metadata as *const ()
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x1, // QEMU will exit with status 3
    Fail = 0x2,    // QEMU will exit with status 5
}

pub fn exit_qemu(exit_code: QemuExitCode) -> ! {
    // https://github.com/qemu/qemu/blob/master/hw/misc/debugexit.c
    x86_64::write_io_port_u8(0xf4, exit_code as u8);
    loop {
        x86_64::hlt();
    }
}
