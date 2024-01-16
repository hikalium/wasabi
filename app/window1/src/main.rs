#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;

use alloc::string::ToString;
use noli::entry_point;
use noli::println;
use noli::window;

fn main() {
    println!("window 1");

    window::Window::new("first window!".to_string(), 0xffffff, 0, 0, 200, 100).unwrap();
    let window2 =
        window::Window::new("second window!!".to_string(), 0xff00ff, 30, 70, 200, 100).unwrap();

    window2.fill_rect(0xff0000, 0, 0, 50, 50).unwrap();
    window2
        .draw_string(
            0x000000,
            5,
            10,
            "test",
            window::StringSize::Large,
            /*underline*/ false,
        )
        .unwrap();
}

entry_point!(main);
