#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]
use noli::prelude::*;
entry_point!(main);

fn main() {
    println!("Hello, world!");
}
