#[derive(Debug, PartialEq, Eq)]
pub enum KeyEvent {
    None,
    Char(char),
    Unknown(u8),
    Enter,
}
impl KeyEvent {
    pub fn from_usb_key_id(usage_id: u8) -> Self {
        match usage_id {
            0 => KeyEvent::None,
            4..=29 => KeyEvent::Char((b'a' + usage_id - 4) as char),
            30..=39 => KeyEvent::Char((b'0' + (usage_id + 1) % 10) as char),
            40 => KeyEvent::Enter,
            42 => KeyEvent::Char(0x08 as char),
            44 => KeyEvent::Char(' '),
            45 => KeyEvent::Char('-'),
            51 => KeyEvent::Char(':'),
            54 => KeyEvent::Char(','),
            55 => KeyEvent::Char('.'),
            56 => KeyEvent::Char('/'),
            _ => KeyEvent::Unknown(usage_id),
        }
    }
    pub fn to_char(&self) -> Option<char> {
        match self {
            KeyEvent::Char(c) => Some(*c),
            KeyEvent::Enter => Some('\n'),
            _ => None,
        }
    }
}
