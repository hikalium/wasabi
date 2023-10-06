#![no_std]
#![no_main]

use noli::*;

fn main() -> i64 {
    println!("window0!!!!");
    println!("window0 {}x{}", 32, 64);
    for y in 0..64 {
        for x in 0..64 {
            sys_draw_point(100 + x, 100 + y, 0xff0000);
        }
    }
    draw_line(0x00ff00, 100, 100, 163, 163).unwrap();
    draw_line(0x00ff00, 163, 100, 100, 163).unwrap();

    draw_rect(0x0000ff, 100, 200, 100, 100).unwrap();
    0
}

entry_point!(main);
