#![no_std]
#![no_main]

use noli::draw_char;
use noli::draw_line;
use noli::draw_rect;
use noli::draw_string;
use noli::draw_string_1p5x;
use noli::draw_string_2x;
use noli::draw_string_3x;
use noli::entry_point;
use noli::fill_circle;
use noli::fill_rect;
use noli::println;
use noli::syscall;

fn main() -> u64 {
    println!("window0!!!!");
    println!("window0 {}x{}", 32, 64);
    for y in 0..64 {
        for x in 0..64 {
            syscall::draw_point(100 + x, 100 + y, 0xff0000);
        }
    }
    draw_line(0x00ff00, 100, 100, 163, 163).unwrap();
    draw_line(0x00ff00, 163, 100, 100, 163).unwrap();

    draw_rect(0x0000ff, 100, 200, 100, 100).unwrap();
    fill_rect(0x0000ff, 250, 200, 100, 100).unwrap();

    fill_circle(0xffff00, 100, 300, 50).unwrap();

    draw_char(0xffffff, 100, 400, 'A').unwrap();
    draw_char(0xffffff, 110, 400, 'b').unwrap();
    draw_char(0xffffff, 120, 400, 'c').unwrap();
    draw_char(0xffffff, 130, 400, '!').unwrap();

    draw_string(0xffffff, 100, 450, "Hello World").unwrap();
    draw_string(0xffffff, 100, 470, "#$*@^&!").unwrap();

    draw_string_1p5x(0xffffff, 100, 500, "Hello World").unwrap();

    draw_string_2x(0xffffff, 100, 540, "Hello World").unwrap();

    draw_string_3x(0xffffff, 100, 600, "Hello World").unwrap();
    0
}

entry_point!(main);
