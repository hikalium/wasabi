#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use noli::prelude::*;

fn main() -> Result<()> {
    println!("# Prints mouse cursor movement");
    println!("# Type q to exit.");
    loop {
        if let Some('q') = Api::read_key() {
            break;
        }
        if Api::get_mouse_cursor_info() {
            println!("!");
        }
    }
    Ok(())
}

entry_point!(main);
