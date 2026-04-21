extern crate alloc;

use crate::keyboard::KeyEvent;
use crate::print;
use alloc::string::String;

#[derive(Default, PartialEq, Eq, Debug)]
pub enum InputEditResult {
    #[default]
    PassThrough,
    UpdatePendingString(String),
    ConfirmString(String),
}

#[derive(Default)]
pub struct InputMethodEditor {
    pending_string: String,
}

impl InputMethodEditor {
    pub fn boin_index(e: char) -> Option<usize> {
        match e {
            'a' => Some(0),
            'i' => Some(1),
            'u' => Some(2),
            'e' => Some(3),
            'o' => Some(4),
            _ => None,
        }
    }
    pub fn shiin_index(e: char) -> Option<usize> {
        match e {
            'k' => Some(0),
            's' => Some(1),
            't' => Some(2),
            'n' => Some(3),
            'h' => Some(4),
            'm' => Some(5),
            'y' => Some(6),
            'r' => Some(7),
            'w' => Some(8),
            // dakuon
            'g' => Some(9),
            'z' => Some(10),
            'd' => Some(11),
            'b' => Some(12),
            // handakuon
            'p' => Some(13),
            _ => None,
        }
    }
    pub fn send_key_down(&mut self, e: KeyEvent) -> InputEditResult {
        match e {
            KeyEvent::Char('\x08') => {
                // Backspace
                if self.pending_string.pop().is_some() {
                    InputEditResult::UpdatePendingString(
                        self.pending_string.clone(),
                    )
                } else {
                    InputEditResult::PassThrough
                }
            }
            KeyEvent::Char(c) => {
                let c = {
                    let prev1_char = c;
                    let prev1 = Self::boin_index(c);
                    let prev2_char = self.pending_string.chars().last();
                    let prev2 = if !self.pending_string.is_empty() {
                        self.pending_string
                            .chars()
                            .last()
                            .and_then(Self::shiin_index)
                    } else {
                        None
                    };
                    if let (_, '-') = (prev2_char, prev1_char) {
                        self.pending_string.pop();
                        'ー'
                    } else if let (Some('n'), 'n') = (prev2_char, prev1_char) {
                        self.pending_string.pop();
                        print!("\x08");
                        'ん'
                    } else {
                        match (prev2, prev1) {
                            (Some(si), Some(bi)) => {
                                self.pending_string.pop();
                                print!("\x08");
                                [
                                    ['か', 'き', 'く', 'け', 'こ'],
                                    ['さ', 'し', 'す', 'せ', 'そ'],
                                    ['た', 'ち', 'つ', 'て', 'と'],
                                    ['な', 'に', 'ぬ', 'ね', 'の'],
                                    ['は', 'ひ', 'ふ', 'へ', 'ほ'],
                                    ['ま', 'み', 'む', 'め', 'も'],
                                    ['や', '　', 'ゆ', '　', 'よ'],
                                    ['ら', 'り', 'る', 'れ', 'ろ'],
                                    ['わ', '　', '　', '　', 'を'],
                                    ['が', 'ぎ', 'ぐ', 'げ', 'ご'],
                                    ['ざ', 'じ', 'ず', 'ぜ', 'ぞ'],
                                    ['だ', 'ぢ', 'づ', 'で', 'ど'],
                                    ['ば', 'び', 'ぶ', 'べ', 'ぼ'],
                                    ['ぱ', 'ぴ', 'ぷ', 'ぺ', 'ぽ'],
                                ][si][bi]
                            }
                            (_, Some(bi)) => ['あ', 'い', 'う', 'え', 'お'][bi],
                            _ => c,
                        }
                    }
                };
                self.pending_string.push(c);
                InputEditResult::UpdatePendingString(
                    self.pending_string.clone(),
                )
            }
            _ => InputEditResult::PassThrough,
        }
    }
}

#[test_case]
fn basic_romaji_conversion() {
    use alloc::string::ToString;
    let mut ime = InputMethodEditor::default();
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('a')),
        InputEditResult::UpdatePendingString("あ".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('i')),
        InputEditResult::UpdatePendingString("あい".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('\x08')),
        InputEditResult::UpdatePendingString("あ".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('k')),
        InputEditResult::UpdatePendingString("あk".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('a')),
        InputEditResult::UpdatePendingString("あか".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('k')),
        InputEditResult::UpdatePendingString("あかk".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('\x08')),
        InputEditResult::UpdatePendingString("あか".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('\x08')),
        InputEditResult::UpdatePendingString("あ".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('\x08')),
        InputEditResult::UpdatePendingString("".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('\x08')),
        InputEditResult::PassThrough
    );
}
