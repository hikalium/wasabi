#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

use noli::entry_point;

fn main() {
    #[allow(clippy::empty_loop)]
    loop {}
}

entry_point!(main);
