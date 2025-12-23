use crate::cui::Console;
use crate::executor::sleep;
use crate::keyboard::KeyEvent;
use crate::result::Result;
use crate::x86::read_io_port_u8;
use core::time::Duration;

const PS2_KEYCODE_US: [KeyEvent; 0x80] = [
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::Char('1'),
    KeyEvent::Char('2'),
    KeyEvent::Char('3'),
    KeyEvent::Char('4'),
    KeyEvent::Char('5'),
    KeyEvent::Char('6'),
    KeyEvent::Char('7'),
    KeyEvent::Char('8'),
    KeyEvent::Char('9'),
    KeyEvent::Char('0'),
    KeyEvent::Char('-'),
    KeyEvent::Char('^'),
    KeyEvent::Char('\x08'),
    KeyEvent::Char('\t'),
    /* 0x10 */
    KeyEvent::Char('q'),
    KeyEvent::Char('w'),
    KeyEvent::Char('e'),
    KeyEvent::Char('r'),
    KeyEvent::Char('t'),
    KeyEvent::Char('y'),
    KeyEvent::Char('u'),
    KeyEvent::Char('i'),
    KeyEvent::Char('o'),
    KeyEvent::Char('p'),
    KeyEvent::Char('@'),
    KeyEvent::Char('['),
    KeyEvent::Enter,
    KeyEvent::None,
    KeyEvent::Char('a'),
    KeyEvent::Char('s'),
    /* 0x20 */
    KeyEvent::Char('d'),
    KeyEvent::Char('f'),
    KeyEvent::Char('g'),
    KeyEvent::Char('h'),
    KeyEvent::Char('j'),
    KeyEvent::Char('k'),
    KeyEvent::Char('l'),
    KeyEvent::Char(';'),
    KeyEvent::Char(':'),
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::Char(']'),
    KeyEvent::Char('z'),
    KeyEvent::Char('x'),
    KeyEvent::Char('c'),
    KeyEvent::Char('v'),
    /* 0x30 */
    KeyEvent::Char('b'),
    KeyEvent::Char('n'),
    KeyEvent::Char('m'),
    KeyEvent::Char(','),
    KeyEvent::Char('.'),
    KeyEvent::Char('/'),
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::Char(' '),
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    /* 0x40 */
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::CursorUp,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::CursorLeft,
    KeyEvent::None,
    KeyEvent::CursorRight,
    KeyEvent::None,
    KeyEvent::None,
    /* 0x50 */
    KeyEvent::CursorDown,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    /* 0x60 */
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    /* 0x70 */
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
    KeyEvent::None,
];

pub async fn ps2kbd_task() -> Result<()> {
    let mut console = Console::default();
    loop {
        let status: u8 = read_io_port_u8(0x64);
        if status & 1 != 0 {
            let value = read_io_port_u8(0x60);
            if value & 0x80 == 0 {
                // Handle key down only for now
                let keycode = match PS2_KEYCODE_US.get(value as usize) {
                    Some(KeyEvent::None) => KeyEvent::Unknown(value),
                    Some(e) => *e,
                    None => KeyEvent::Unknown(value),
                };
                console.handle_key_down(keycode);
            }
        }
        sleep(Duration::from_millis(10)).await;
    }
}
