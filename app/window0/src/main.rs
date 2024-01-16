#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

use noli::entry_point;
use noli::graphics;
use noli::println;
use noli::sys::draw_point;

fn main() {
    println!("window0!!!!");
    println!("window0 {}x{}", 32, 64);
    for y in 0..64 {
        for x in 0..64 {
            draw_point(100 + x, 100 + y, 0xff0000);
        }
    }
    graphics::draw_line(0x00ff00, 100, 100, 163, 163).unwrap();
    graphics::draw_line(0x00ff00, 163, 100, 100, 163).unwrap();

    graphics::draw_rect(0x0000ff, 100, 200, 100, 100).unwrap();
    graphics::fill_rect(0x0000ff, 250, 200, 100, 100).unwrap();

    graphics::fill_circle(0xffff00, 100, 300, 50).unwrap();

    graphics::draw_char(0xffffff, 100, 400, 'A').unwrap();
    graphics::draw_char(0xffffff, 110, 400, 'b').unwrap();
    graphics::draw_char(0xffffff, 120, 400, 'c').unwrap();
    graphics::draw_char(0xffffff, 130, 400, '!').unwrap();

    graphics::draw_string(0xffffff, 100, 450, "Hello World").unwrap();
    graphics::draw_string(0xffffff, 100, 470, "#$*@^&!").unwrap();

    graphics::draw_string_1p5x(0xffffff, 100, 500, "Hello World").unwrap();

    graphics::draw_string_2x(0xffffff, 100, 540, "Hello World").unwrap();

    graphics::draw_string_3x(0xffffff, 100, 600, "Hello World").unwrap();
}

entry_point!(main);
