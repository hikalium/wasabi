#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use alloc::string::String;
use noli::prelude::*;

fn main() -> Result<()> {
    let args = Api::get_args_region();
    println!("{args:?}");
    let mut line = String::new();
    println!("# Reverses the input string on a row.");
    println!("# Type q and hit Enter to exit.");
    loop {
        if let Some(c) = Api::read_key() {
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
