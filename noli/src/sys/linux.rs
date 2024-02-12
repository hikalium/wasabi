extern crate std;

use std::print;

pub fn exit(code: u64) -> ! {
    std::process::exit(code as i32);
}
pub fn write_string(s: &str) -> u64 {
    print!("{s}");
    s.len() as u64
}
pub fn draw_point(_x: i64, _y: i64, _c: u32) -> u64 {
    // We don't support GUI for Linux targets as it will only be used for unit testing
    0
}
pub fn noop() -> u64 {
    // Do nothing
    0
}

pub fn read_key() -> Option<char> {
    unimplemented!()
}

#[macro_export]
macro_rules! entry_point {
    ($path:path) => {
        // Do nothing
        #[allow(unused_must_use)]
        pub fn stub() {
            // reference main to avoid "unused" error
            main();
        }
    };
}
