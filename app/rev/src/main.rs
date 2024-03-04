#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use alloc::string::String;
use noli::entry_point;
use noli::error::Result;
use noli::graphics::*;
use noli::print;
use noli::println;

fn main() -> Result<()> {
    let mut line = String::new();
    println!("# Reverses the input string on a row.");
    println!("# Type q and hit Enter to exit.");
    loop {
        if let Some(c) = noli::sys::read_key() {
            print!("{c}");
            if c == '\n' {
                if line == "q" {
                    println!("bye!");
                    break;
                }
                let reversed = line.chars().rev().collect::<String>();
                println!("{reversed}");
                line.clear();
            } else {
                line.push(c);
            }
        }
    }
    Ok(())
}

entry_point!(main);
