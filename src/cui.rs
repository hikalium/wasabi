extern crate alloc;

use crate::keyboard::KeyEvent;
use crate::print;
use crate::println;
use crate::warn;
use alloc::string::String;
use core::mem::swap;

#[derive(Default)]
pub struct Console {
    prev_cmd: Option<String>,
    input_buf: String,
}
impl Console {
    pub fn handle_key_down(&mut self, e: KeyEvent) {
        match e {
            KeyEvent::Char('\x08') => {
                self.input_buf.pop();
                print!("\x08");
            }
            KeyEvent::Char(c) => {
                self.input_buf.push(c);
                print!("{c}");
            }
            KeyEvent::CursorUp => {
                if let Some(prev_cmd) = self.prev_cmd.as_mut() {
                    swap(prev_cmd, &mut self.input_buf);
                    print!("\n{}", self.input_buf);
                }
            }
            KeyEvent::Enter => {
                println!("\n{}", self.input_buf);
                let mut prev_cmd = String::new();
                swap(&mut prev_cmd, &mut self.input_buf);
                self.prev_cmd = Some(prev_cmd);
            }
            e => warn!("Unhandled input: {e:?}"),
        }
    }
}
