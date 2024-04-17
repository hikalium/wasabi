#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use noli::graphics;
use noli::prelude::*;
use sabi::PointerPosition;

fn main() -> Result<()> {
    println!("# Prints mouse cursor movement");
    println!("# Type q to exit.");
    let mut last_position: Option<PointerPosition> = None;
    loop {
        if let Some('q') = Api::read_key() {
            break;
        }
        if let Some(MouseEvent { button, position }) = Api::get_mouse_cursor_info() {
            println!("{button:?} {position:?}");
            if button.l() || button.c() || button.r() {
                let color = ((button.l() as u32) * 0xff0000)
                    | ((button.c() as u32) * 0x00ff00)
                    | ((button.r() as u32) * 0x0000ff);
                if let Some(last_position) = last_position {
                    graphics::draw_line(
                        color,
                        last_position.x,
                        last_position.y,
                        position.x,
                        position.y,
                    )
                    .unwrap();
                } else {
                    graphics::draw_point(0x00ff00, position.x, position.y).unwrap();
                }
                last_position = Some(position);
            } else {
                last_position = None;
            }
        }
    }
    Ok(())
}

entry_point!(main);
