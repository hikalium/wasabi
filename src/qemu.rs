use crate::x86::hlt;
use crate::x86::write_io_port_u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x1, // QEMU will exit with status 3
    Fail = 0x2,    // QEMU will exit with status 5
}
pub fn exit_qemu(exit_code: QemuExitCode) -> ! {
    // https://github.com/qemu/qemu/blob/master/hw/misc/debugexit.c
    write_io_port_u8(0xf4, exit_code as u8);
    loop {
        hlt()
    }
}
