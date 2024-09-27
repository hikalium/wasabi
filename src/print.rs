use crate::serial::SerialPort;
use core::fmt;

pub fn global_print(args: fmt::Arguments) {
    let mut writer = SerialPort::default();
    fmt::write(&mut writer, args).unwrap();
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
            ($($arg:tt)*) => ($crate::print!("[INFO]  {}:{:<3}: {}\n", file!(), line!(), format_args!($($arg)*)));
}

#[macro_export]
macro_rules! warn {
            ($($arg:tt)*) => ($crate::print!("[WARN]  {}:{:<3}: {}\n", file!(), line!(), format_args!($($arg)*)));
}

#[macro_export]
macro_rules! error {
            ($($arg:tt)*) => ($crate::print!("[ERROR] {}:{:<3}: {}\n", file!(), line!(), format_args!($($arg)*)));
}
