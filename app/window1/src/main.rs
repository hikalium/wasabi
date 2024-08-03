#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;

use alloc::string::ToString;
use noli::entry_point;
use noli::println;
use noli::window;

fn main() {
    println!("window 1");

    window::Window::new("first window!".to_string(), 0xffffff, 30, 30, 200, 100).unwrap();
    let mut window2 =
        window::Window::new("second window!!".to_string(), 0xff00ff, 60, 100, 200, 100).unwrap();
    let mut window3 =
        window::Window::new("third window!!".to_string(), 0x0000ff, 90, 170, 200, 100).unwrap();

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

    assert!(window3.fill_rect(0xff0000, 50, 50, 50, 26).is_ok());
    // try to fill a bigger rect than the size of the window.
    assert!(window3.fill_rect(0xff0000, 100, 50, 50, 27).is_err());
}

entry_point!(main);
