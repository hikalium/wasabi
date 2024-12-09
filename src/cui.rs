extern crate alloc;

use crate::keyboard::KeyEvent;
use crate::print;
use crate::println;
use crate::warn;
use alloc::string::String;

#[derive(Default)]
pub struct Console {
    input_buf: String,
}
impl Console {
    pub fn handle_key_down(&mut self, e: KeyEvent) {
        match e {
            KeyEvent::Char(c) => {
                self.input_buf.push(c);
                print!("{c}");
            }
            KeyEvent::Enter => {
                println!("\n{}", self.input_buf);
                self.input_buf.clear();
            }
            e => warn!("Unhandled input: {e:?}"),
        }
    }
}
