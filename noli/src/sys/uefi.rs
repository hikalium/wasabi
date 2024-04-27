use crate::sys::api::SystemApi;

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
    fn exit(_code: u64) -> ! {
        unimplemented!()
    }
    fn write_string(_s: &str) -> u64 {
        unimplemented!()
    }
    /// We don't support GUI for Linux targets as it will only be used for unit testing
    fn draw_point(_x: i64, _y: i64, _c: u32) -> u64 {
        0
    }
}
