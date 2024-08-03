#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;

use alloc::string::ToString;
use noli::bitmap::bitmap_draw_rect;
use noli::entry_point;
use noli::prelude::SystemApi;
use noli::rect::Rect;
use noli::sheet::Sheet;
use noli::sys::api::MouseEvent;
use noli::sys::wasabi::Api;
use noli::window;

fn main() {
    let mut window =
        window::Window::new("first window!".to_string(), 0xffffff, 50, 100, 200, 100).unwrap();
    window.fill_rect(0x0000ff, 0, 0, 50, 50).unwrap();
    window.flush();

    let mut cursor = Sheet::new(Rect::new(0, 0, 20, 20).unwrap());
    let bitmap = cursor.bitmap();
    let _ = bitmap_draw_rect(bitmap, 0xff0000, 0, 0, 20, 20);

    loop {
        if let Some(MouseEvent { button, position }) = Api::get_mouse_cursor_info() {
            window.flush_area(cursor.rect());
            cursor.set_position(position.x, position.y);
            window.flush_area(cursor.rect());
            cursor.flush();

            if button.l() || button.c() || button.r() {
                window.flush();
            }
        }
    }
}

entry_point!(main);
