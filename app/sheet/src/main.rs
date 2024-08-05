#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

use noli::bitmap::bitmap_draw_rect;
use noli::bitmap::Bitmap;
use noli::prelude::*;
use noli::rect::Rect;
use noli::sheet::Sheet;

fn main() {
    let mut s1 = Sheet::new(Rect::new(100, 100, 200, 200).unwrap());
    let mut s2 = Sheet::new(Rect::new(150, 150, 200, 200).unwrap());

    let b1 = s1.bitmap();
    let _ = bitmap_draw_rect(b1, 0x0000ff, 0, 0, b1.width(), b1.height());
    s1.flush();

    let b2 = s2.bitmap();
    let _ = bitmap_draw_rect(b2, 0xff00ff, 0, 0, b2.width(), b2.height());
    s2.flush();

    s1.flush();

    let _ = s1.draw_border(0x00ff00);
    let _ = s2.draw_border(0xff0000);
}

entry_point!(main);
