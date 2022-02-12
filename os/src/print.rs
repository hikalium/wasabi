use crate::text_area::*;
use crate::vram::VRAMBufferInfo;
use core::cell::RefCell;
use core::fmt;

pub struct GlobalPrinter {
    text_area: RefCell<Option<TextArea<VRAMBufferInfo>>>,
}

impl GlobalPrinter {
    pub fn set_text_area(&self, text_area: TextArea<VRAMBufferInfo>) {
        *self.text_area.borrow_mut() = Some(text_area);
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
macro_rules! println {
        () => ($crate::print!("\n"));
            ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut writer = crate::serial::SerialConsoleWriter {};
    fmt::write(&mut writer, args).unwrap();
    match &mut *GLOBAL_PRINTER.text_area.borrow_mut() {
        Some(w) => fmt::write(w, args).unwrap(),
        None => {}
    }
}
