use crate::println;
use crate::serial::SerialConsoleWriter;
use crate::text_area::*;
use crate::vram::VRAMBufferInfo;
use core::cell::RefCell;
use core::fmt;
use core::mem::size_of;
use core::slice;

pub struct GlobalPrinter {
    text_area: RefCell<Option<TextArea<VRAMBufferInfo>>>,
}

impl GlobalPrinter {
    pub fn set_text_area(&self, text_area: TextArea<VRAMBufferInfo>) {
        *self.text_area.borrow_mut() = Some(text_area);
        println!("GlobalPrinter: TextArea is set. Printing to VRAM.");
    }
}

/// # Safety
///
/// This is safe since we don't have multi-tasking yet.
unsafe impl Sync for GlobalPrinter {}

pub static GLOBAL_PRINTER: GlobalPrinter = GlobalPrinter {
    text_area: RefCell::new(None),
};

#[macro_export]
macro_rules! print {
        ($($arg:tt)*) => ($crate::print::_print(format_args!($($arg)*)));
}
#[macro_export]
macro_rules! print_nothing {
        ($($arg:tt)*) => ($crate::print::_print_nothing(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
        () => ($crate::print!("\n"));
            ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut writer = SerialConsoleWriter::default();
    fmt::write(&mut writer, args).unwrap();
    match &mut *GLOBAL_PRINTER.text_area.borrow_mut() {
        Some(w) => fmt::write(w, args).unwrap(),
        None => {}
    }
}
#[doc(hidden)]
pub fn _print_nothing(_args: fmt::Arguments) {}

pub fn hexdump(bytes: &[u8]) {
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

pub fn hexdump_struct<T>(data: &T) {
    hexdump(unsafe { slice::from_raw_parts(data as *const T as *const u8, size_of::<T>()) })
}
