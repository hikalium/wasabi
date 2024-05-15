#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use noli::args;
use noli::prelude::*;

fn main() -> Result<()> {
    let args = args::from_env();
    println!("{args:?}");
    Ok(())
}

entry_point!(main);
