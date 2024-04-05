#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

use noli::prelude::*;

fn main() {
    Api::write_string("Hello hikalium! This is wasabi app!\n");
}
entry_point!(main);
