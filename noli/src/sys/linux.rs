extern crate std;

use crate::sys::api::SystemApi;

use std::print;

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

pub struct Api;

impl SystemApi for Api {
    fn exit(code: u64) -> ! {
        std::process::exit(code as i32)
    }
    fn write_string(s: &str) -> u64 {
        print!("{s}");
        s.len() as u64
    }
    /// We don't support GUI for Linux targets as it will only be used for unit testing
    fn draw_point(_x: i64, _y: i64, _c: u32) -> u64 {
        0
    }
}
