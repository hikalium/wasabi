// c.f. https://github.com/hikalium/wasabi/pull/24
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
#[cfg_attr(target_os = "linux", no_main)]
use noli::prelude::*;
entry_point!(main);

fn main() -> u64 {
    let mut v: Vec<usize> = Vec::new();
    v.reserve(1); // This allocation is no problem but this increases segment size significantly because it requires noli's ALLOCATOR
    0
}
