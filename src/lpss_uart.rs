use crate::mutex::Mutex;
use crate::x86::busy_loop_hint;
use core::fmt;
use core::ptr::read_volatile;
use core::ptr::write_volatile;

// A DW_apb_uart (Synopsys DesignWare) behind Intel's LPSS, as found on
// Chromebooks, where it carries the AP console the GSC exposes over
// Closed Case Debugging. It keeps the 16550 register layout but puts
// the registers on a 32-bit grid — register n lives at base + n * 4,
// the `regshift = 2` that Linux sets in 8250_lpss.c — so this cannot
// share the byte-addressed `SerialPort` used for COM1.

// Register indices, to be scaled by the 32-bit stride.
const REG_DATA: usize = 0; // RBR / THR, or DLL while DLAB is set
const REG_IER: usize = 1; // or DLH while DLAB is set
const REG_FCR: usize = 2;
const REG_LCR: usize = 3;
const REG_MCR: usize = 4;
const REG_LSR: usize = 5;

// DesignWare extensions, addressed by byte offset rather than by
// register index.
const OFF_UCV: usize = 0xf8; // UART Component Version

const LSR_DATA_READY: u8 = 0x01;
const LSR_THR_EMPTY: u8 = 0x20;

// Give up rather than hang if the transmitter never drains: this is on
// the path of every `print!`, so a dead port must not take the OS with
// it.
const TX_WAIT_LIMIT: usize = 100_000;

/// The port the kernel prints to and reads commands from, once
/// `init_lpss_uart` has found one. Copied out before use so that no
/// lock is held across the MMIO — `print!` reaches here from anywhere,
/// including from the task draining the port.
static LPSS_UART: Mutex<Option<LpssUart>> = Mutex::new(None);

pub fn lpss_uart() -> Option<LpssUart> {
    *LPSS_UART.lock()
}

/// Probe `base`, and on success configure the port and make it the
/// global one. Returns whether a port was found.
pub fn init_lpss_uart(base: usize) -> bool {
    match LpssUart::probe(base) {
        Some(uart) => {
            uart.init();
            *LPSS_UART.lock() = Some(uart);
            true
        }
        None => false,
    }
}

#[derive(Copy, Clone)]
pub struct LpssUart {
    base: usize,
}
impl LpssUart {
    fn reg(&self, index: usize) -> *mut u8 {
        (self.base + index * 4) as *mut u8
    }
    /// Read the Component Version register to decide whether there is a
    /// DesignWare UART here at all. An absent or unmapped window reads
    /// back as all-zeroes or all-ones, neither of which is a version.
    /// Linux makes the same call in `dw8250_setup_port`.
    pub fn probe(base: usize) -> Option<Self> {
        let ucv = unsafe { read_volatile((base + OFF_UCV) as *const u32) };
        if ucv == 0 || ucv == u32::MAX {
            return None;
        }
        Some(Self { base })
    }
    pub fn init(&self) {
        unsafe {
            // The LPSS fractional divider feeds this port 1.8432MHz, so
            // a divisor of 1 gives 1843200 / 16 = 115200 baud, which is
            // what the GSC expects on the AP console.
            write_volatile(self.reg(REG_LCR), 0x83); // DLAB, 8N1
            write_volatile(self.reg(REG_DATA), 0x01); // DLL
            write_volatile(self.reg(REG_IER), 0x00); // DLH
            write_volatile(self.reg(REG_LCR), 0x03); // 8N1, DLAB off
            write_volatile(self.reg(REG_IER), 0x00); // polled, no interrupts
            write_volatile(self.reg(REG_FCR), 0xC7); // enable, clear fifos
            write_volatile(self.reg(REG_MCR), 0x0B); // DTR, RTS, OUT2
        }
    }
    pub fn try_read(&self) -> Option<u8> {
        let lsr = unsafe { read_volatile(self.reg(REG_LSR)) };
        if lsr & LSR_DATA_READY == 0 {
            None
        } else {
            Some(unsafe { read_volatile(self.reg(REG_DATA)) })
        }
    }
    pub fn send_byte(&self, b: u8) {
        for _ in 0..TX_WAIT_LIMIT {
            let lsr = unsafe { read_volatile(self.reg(REG_LSR)) };
            if lsr & LSR_THR_EMPTY != 0 {
                unsafe { write_volatile(self.reg(REG_DATA), b) };
                return;
            }
            busy_loop_hint();
        }
    }
    pub fn send_str(&self, s: &str) {
        for b in s.bytes() {
            // The console this feeds is a terminal, which wants CRLF.
            if b == b'\n' {
                self.send_byte(b'\r');
            }
            self.send_byte(b);
        }
    }
}
impl fmt::Write for LpssUart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.send_str(s);
        Ok(())
    }
}
