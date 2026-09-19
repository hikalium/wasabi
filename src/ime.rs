extern crate alloc;

use crate::keyboard::KeyEvent;
use alloc::string::String;
use alloc::string::ToString;

#[derive(Default, PartialEq, Eq, Debug)]
pub enum InputEditResult {
    #[default]
    PassThrough,
    UpdatePendingString(String),
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

    pub fn is_consonant(e: char) -> bool {
        e.is_ascii_lowercase() && Self::boin_index(e).is_none()
    }
    /// The text typed so far that has not been committed yet.
    pub fn pending(&self) -> &str {
        &self.pending_string
    }
    /// Hands the pending text over to the caller for committing,
    /// leaving the IME empty.
    pub fn take_pending(&mut self) -> String {
        core::mem::take(&mut self.pending_string)
    }
    // The kana row (a, i, u, e, o) that a consonant spells. An empty
    // string means the row has no kana for that vowel.
    fn romaji_row2(c: char) -> Option<[&'static str; 5]> {
        match c {
            'k' => Some(["か", "き", "く", "け", "こ"]),
            's' => Some(["さ", "し", "す", "せ", "そ"]),
            't' => Some(["た", "ち", "つ", "て", "と"]),
            'n' => Some(["な", "に", "ぬ", "ね", "の"]),
            'h' => Some(["は", "ひ", "ふ", "へ", "ほ"]),
            'm' => Some(["ま", "み", "む", "め", "も"]),
            'y' => Some(["や", "", "ゆ", "いぇ", "よ"]),
            'r' => Some(["ら", "り", "る", "れ", "ろ"]),
            'w' => Some(["わ", "うぃ", "う", "うぇ", "を"]),
            'g' => Some(["が", "ぎ", "ぐ", "げ", "ご"]),
            'z' => Some(["ざ", "じ", "ず", "ぜ", "ぞ"]),
            'd' => Some(["だ", "ぢ", "づ", "で", "ど"]),
            'b' => Some(["ば", "び", "ぶ", "べ", "ぼ"]),
            'p' => Some(["ぱ", "ぴ", "ぷ", "ぺ", "ぽ"]),
            'j' => Some(["じゃ", "じ", "じゅ", "じぇ", "じょ"]),
            _ => None,
        }
    }
    // The kana row that a pair of consonants spells, such as ky or sh.
    fn romaji_row3(c1: char, c2: char) -> Option<[&'static str; 5]> {
        match (c1, c2) {
            ('k', 'y') => Some(["きゃ", "きぃ", "きゅ", "きぇ", "きょ"]),
            ('s', 'y') => Some(["しゃ", "しぃ", "しゅ", "しぇ", "しょ"]),
            ('s', 'h') => Some(["しゃ", "し", "しゅ", "しぇ", "しょ"]),
            ('c', 'h') => Some(["ちゃ", "ち", "ちゅ", "ちぇ", "ちょ"]),
            ('t', 'y') => Some(["ちゃ", "ち", "ちゅ", "ちぇ", "ちょ"]),
            ('n', 'y') => Some(["にゃ", "にぃ", "にゅ", "にぇ", "にょ"]),
            ('h', 'y') => Some(["ひゃ", "ひぃ", "ひゅ", "ひぇ", "ひょ"]),
            ('m', 'y') => Some(["みゃ", "みぃ", "みゅ", "みぇ", "みょ"]),
            ('r', 'y') => Some(["りゃ", "りぃ", "りゅ", "りぇ", "りょ"]),
            ('g', 'y') => Some(["ぎゃ", "ぎぃ", "ぎゅ", "ぎぇ", "ぎょ"]),
            ('d', 'y') => Some(["ぢゃ", "ぢぃ", "ぢゅ", "ぢぇ", "ぢょ"]),
            ('b', 'y') => Some(["びゃ", "びぃ", "びゅ", "びぇ", "びょ"]),
            ('p', 'y') => Some(["ぴゃ", "ぴぃ", "ぴゅ", "ぴぇ", "ぴょ"]),
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
            KeyEvent::Char(prev1) => {
                let s = {
                    let (prev3, prev2) = {
                        let mut it = self.pending_string.chars().rev();
                        let prev2 = it.next();
                        let prev3 = it.next();
                        (prev3, prev2)
                    };
                    if '-' == prev1 {
                        String::from('ー')
                    } else if ' ' == prev1 {
                        if self.pending_string == "きょう" {
                            self.pending_string.pop();
                            self.pending_string.pop();
                            self.pending_string.pop();
                            "今日".to_string()
                        } else {
                            "　".to_string()
                        }
                    } else if let (Some('n'), 'n') = (prev2, prev1) {
                        self.pending_string.pop();
                        String::from('ん')
                    } else if prev2
                        .map(|prev2| {
                            prev2 == prev1 && Self::is_consonant(prev1)
                        })
                        .unwrap_or_default()
                    {
                        self.pending_string.pop();
                        String::from('っ') + &String::from(prev1)
                    } else if let Some(boin_index) = Self::boin_index(prev1) {
                        let mut after = None;
                        if let (Some(prev3), Some(prev2)) = (prev3, prev2) {
                            if let Some(res) = Self::romaji_row3(prev3, prev2) {
                                let res = res[boin_index];
                                if !res.is_empty() {
                                    after = Some(res);
                                    self.pending_string.pop();
                                    self.pending_string.pop();
                                }
                            }
                        }
                        if after.is_none() {
                            if let Some(prev2) = prev2 {
                                if let Some(res) = Self::romaji_row2(prev2) {
                                    let res = res[boin_index];
                                    if !res.is_empty() {
                                        after = Some(res);
                                        self.pending_string.pop();
                                    }
                                }
                            }
                        }
                        if after.is_none() {
                            after = Some(
                                ["あ", "い", "う", "え", "お"][boin_index],
                            );
                        }
                        // after: Option<&str>
                        if let Some(after) = after {
                            after.to_string()
                        } else {
                            String::from(prev1)
                        }
                    } else {
                        String::from(prev1)
                    }
                };
                self.pending_string += &s;
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
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('t')),
        InputEditResult::UpdatePendingString("t".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('y')),
        InputEditResult::UpdatePendingString("ty".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('a')),
        InputEditResult::UpdatePendingString("ちゃ".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('t')),
        InputEditResult::UpdatePendingString("ちゃt".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('t')),
        InputEditResult::UpdatePendingString("ちゃっt".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('o')),
        InputEditResult::UpdatePendingString("ちゃっと".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('n')),
        InputEditResult::UpdatePendingString("ちゃっとn".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('n')),
        InputEditResult::UpdatePendingString("ちゃっとん".to_string())
    );
}

#[test_case]
fn today_conversion() {
    use alloc::string::ToString;
    let mut ime = InputMethodEditor::default();
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('k')),
        InputEditResult::UpdatePendingString("k".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('y')),
        InputEditResult::UpdatePendingString("ky".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('o')),
        InputEditResult::UpdatePendingString("きょ".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char('u')),
        InputEditResult::UpdatePendingString("きょう".to_string())
    );
    assert_eq!(
        ime.send_key_down(KeyEvent::Char(' ')),
        InputEditResult::UpdatePendingString("今日".to_string())
    );
}
