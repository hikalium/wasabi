use crate::graphics::Bitmap;
use crate::graphics::BitmapTextWriter;
use crate::mutex::Mutex;
use crate::serial::SerialPort;
use crate::tcp::TCP_SOCKET;
use crate::uefi::VramBufferInfo;
use core::fmt;
use core::mem::size_of;
use core::slice;

static GLOBAL_VRAM_WRITER: Mutex<Option<BitmapTextWriter<VramBufferInfo>>> =
    Mutex::new(None);
pub fn set_global_vram(vram: VramBufferInfo) {
    assert!(GLOBAL_VRAM_WRITER.lock().is_none());
    let w = BitmapTextWriter::new(vram);
    *GLOBAL_VRAM_WRITER.lock() = Some(w);
}
// Temporary accessor for drawing directly on the global VRAM before the
// GUI layer exists; removed when GLOBAL_VRAM moves into gui.rs.
pub fn with_global_vram_buf(f: impl FnOnce(&mut VramBufferInfo)) {
    if let Some(w) = &mut *GLOBAL_VRAM_WRITER.lock() {
        f(w.buf_mut());
    }
}
pub fn get_global_vram_resolutions() -> Option<(i64, i64)> {
    (GLOBAL_VRAM_WRITER.lock())
        .as_ref()
        .map(|vram| (vram.buf().width(), vram.buf().height()))
}
// Mirrors `print!`/`println!` output into the TCP socket's tx queue
// when a connection is Established. `push_tx_bytes` is itself a no-op
// in any other state, so this is safe to call unconditionally.
struct TcpMirror;
impl fmt::Write for TcpMirror {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        TCP_SOCKET.push_tx_bytes(s.as_bytes());
        Ok(())
    }
}

pub fn global_print(args: fmt::Arguments) {
    let mut writer = SerialPort::default();
    fmt::write(&mut writer, args).unwrap();
    if let Some(w) = &mut *GLOBAL_VRAM_WRITER.lock() {
        fmt::write(w, args).expect("Failed to write to GLOBAL_VRAM_WRITER");
    }
    let _ = fmt::write(&mut TcpMirror, args);
}

/// Print path used from the `#[panic_handler]`. Always reaches the
/// serial port (no Mutex), and uses `try_lock` for the lockable sinks
/// — if a sink's lock is held by the very chain that's now panicking,
/// we skip that sink instead of spinning into a recursive panic that
/// would triple-fault the box.
pub fn panic_print(args: fmt::Arguments) {
    let mut writer = SerialPort::default();
    let _ = fmt::write(&mut writer, args);
    match GLOBAL_VRAM_WRITER.try_lock() {
        Ok(mut printer) => {
            if let Some(printer) = &mut *printer {
                let _ = fmt::write(printer, args);
            }
        }
        Err(_) => {
            let _ = fmt::write(
                &mut writer,
                format_args!(
                    "[panic_print] GLOBAL_VRAM_WRITER is already locked — \
                     panic likely originated inside the print path; \
                     screen output skipped\n"
                ),
            );
        }
    }
    // Skip the TCP mirror entirely: `push_tx_bytes` itself uses a
    // blocking lock and would defeat the point of panic_print. Serial
    // is the authoritative place to look for a panic message anyway.
}

#[macro_export]
macro_rules! print {
        ($($arg:tt)*) => ($crate::print::global_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
        () => ($crate::print!("\n"));
            ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[macro_export]
macro_rules! info {
            ($($arg:tt)*) => ($crate::print!("[INFO]  {}:{:<3}: {}\n",
                    file!(), line!(), format_args!($($arg)*)));
}

#[macro_export]
macro_rules! warn {
            ($($arg:tt)*) => ($crate::print!("[WARN]  {}:{:<3}: {}\n",
                    file!(), line!(), format_args!($($arg)*)));
}

#[macro_export]
macro_rules! error {
            ($($arg:tt)*) => ($crate::print!("[ERROR] {}:{:<3}: {}\n",
                    file!(), line!(), format_args!($($arg)*)));
}

pub fn hexdump_bytes(bytes: &[u8]) {
    let mut i = 0;
    let mut ascii = [0u8; 16];
    let mut offset = 0;
    for v in bytes.iter() {
        if i == 0 {
            print!("{offset:08X}: ");
        }
        print!("{:02X} ", v);
        ascii[i] = *v;
        i += 1;
        if i == 16 {
            print!("|");
            for c in ascii.iter() {
                print!(
                    "{}",
                    match c {
                        0x20..=0x7e => {
                            *c as char
                        }
                        _ => {
                            '.'
                        }
                    }
                );
            }
            println!("|");
            offset += 16;
            i = 0;
        }
    }
    if i != 0 {
        let old_i = i;
        while i < 16 {
            print!("   ");
            i += 1;
        }
        print!("|");
        for c in ascii[0..old_i].iter() {
            print!(
                "{}",
                if (0x20u8..=0x7fu8).contains(c) {
                    *c as char
                } else {
                    '.'
                }
            );
        }
        println!("|");
    }
}
pub fn hexdump_struct<T: Sized>(data: &T) {
    info!("hexdump_struct: {:?}", core::any::type_name::<T>());
    hexdump_bytes(unsafe {
        slice::from_raw_parts(data as *const T as *const u8, size_of::<T>())
    })
}
