#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use noli::entry_point;
use noli::error::Result;
use noli::println;

fn main() -> Result<()> {
    let c = char::from_u32(noli::sys::read_key() as u32);
    println!("{c:?}");
    Ok(())
}

entry_point!(main);
