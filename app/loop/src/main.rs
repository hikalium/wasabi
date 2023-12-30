#![no_std]
#![no_main]

use noli::entry_point;

fn main() -> u64 {
    #[allow(clippy::empty_loop)]
    loop {}
}

entry_point!(main);
