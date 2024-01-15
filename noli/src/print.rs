use core::fmt;

#[macro_export]
macro_rules! print {
        ($($arg:tt)*) => ($crate::print::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    // Note: exported macros will be exposed as the crate's root item.
    // c.f. https://doc.rust-lang.org/reference/macros-by-example.html#path-based-scope
        () => ($crate::print!("\n"));
            ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

pub struct StdIoWriter {}
impl StdIoWriter {}
impl fmt::Write for StdIoWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::syscall::print(s);
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let mut writer = crate::print::StdIoWriter {};
    fmt::write(&mut writer, args).unwrap();
}
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
