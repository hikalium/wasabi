#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use noli::entry_point;
use noli::error::Result;
use noli::println;

fn main() -> Result<()> {
    println!("# Prints mouse cursor movement");
    println!("# Type q to exit.");
    loop {
        if let Some('q') = noli::sys::read_key() {
            break;
        }
        if noli::sys::get_mouse_cursor_info() {
            println!("!");
        }
    }
    Ok(())
}

entry_point!(main);
