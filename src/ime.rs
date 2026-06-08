extern crate alloc;

use crate::keyboard::KeyEvent;
use alloc::collections::BTreeMap;
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
    romaji_map2: BTreeMap<char, [&'static str; 5]>,
    romaji_map3: BTreeMap<(char, char), [&'static str; 5]>,
}

impl InputMethodEditor {
    pub fn init_romaji_map(&mut self) {
        self.romaji_map2.insert('k', ["か", "き", "く", "け", "こ"]);
        self.romaji_map2.insert('s', ["さ", "し", "す", "せ", "そ"]);
        self.romaji_map2.insert('t', ["た", "ち", "つ", "て", "と"]);
        self.romaji_map2.insert('n', ["な", "に", "ぬ", "ね", "の"]);
        self.romaji_map2.insert('h', ["は", "ひ", "ふ", "へ", "ほ"]);
        self.romaji_map2.insert('m', ["ま", "み", "む", "め", "も"]);
        self.romaji_map2.insert('y', ["や", "", "ゆ", "いぇ", "よ"]);
        self.romaji_map2.insert('r', ["ら", "り", "る", "れ", "ろ"]);
        self.romaji_map2
            .insert('w', ["わ", "うぃ", "う", "うぇ", "を"]);
        self.romaji_map2.insert('g', ["が", "ぎ", "ぐ", "げ", "ご"]);
        self.romaji_map2.insert('z', ["ざ", "じ", "ず", "ぜ", "ぞ"]);
        self.romaji_map2.insert('d', ["だ", "ぢ", "づ", "で", "ど"]);
        self.romaji_map2.insert('b', ["ば", "び", "ぶ", "べ", "ぼ"]);
        self.romaji_map2.insert('p', ["ぱ", "ぴ", "ぷ", "ぺ", "ぽ"]);
        self.romaji_map2
            .insert('j', ["じゃ", "じ", "じゅ", "じぇ", "じょ"]);
        self.romaji_map2.insert('d', ["だ", "ぢ", "づ", "で", "ど"]);

        self.romaji_map3
            .insert(('k', 'y'), ["きゃ", "きぃ", "きゅ", "きぇ", "きょ"]);
        self.romaji_map3
            .insert(('s', 'y'), ["しゃ", "しぃ", "しゅ", "しぇ", "しょ"]);
        self.romaji_map3
            .insert(('s', 'h'), ["しゃ", "し", "しゅ", "しぇ", "しょ"]);
        self.romaji_map3
            .insert(('c', 'h'), ["ちゃ", "ち", "ちゅ", "ちぇ", "ちょ"]);
        self.romaji_map3
            .insert(('t', 'y'), ["ちゃ", "ち", "ちゅ", "ちぇ", "ちょ"]);
        self.romaji_map3
            .insert(('n', 'y'), ["にゃ", "にぃ", "にゅ", "にぇ", "にょ"]);
        self.romaji_map3
            .insert(('h', 'y'), ["ひゃ", "ひぃ", "ひゅ", "ひぇ", "ひょ"]);
        self.romaji_map3
            .insert(('m', 'y'), ["みゃ", "みぃ", "みゅ", "みぇ", "みょ"]);
        self.romaji_map3
            .insert(('r', 'y'), ["りゃ", "りぃ", "りゅ", "りぇ", "りょ"]);
        self.romaji_map3
            .insert(('g', 'y'), ["ぎゃ", "ぎぃ", "ぎゅ", "ぎぇ", "ぎょ"]);
        self.romaji_map3
            .insert(('d', 'y'), ["ぢゃ", "ぢぃ", "ぢゅ", "ぢぇ", "ぢょ"]);
        self.romaji_map3
            .insert(('b', 'y'), ["びゃ", "びぃ", "びゅ", "びぇ", "びょ"]);
        self.romaji_map3
            .insert(('p', 'y'), ["ぴゃ", "ぴぃ", "ぴゅ", "ぴぇ", "ぴょ"]);
    }
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
    /// Replace the pending (pre-conversion) string. Used by callers to
    /// keep the IME in sync after they reset or reload the input line.
    pub fn set_pending(&mut self, s: String) {
        self.pending_string = s;
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
                            if let Some(res) =
                                self.romaji_map3.get(&(prev3, prev2))
                            {
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
                                if let Some(res) = self.romaji_map2.get(&prev2)
                                {
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
    ime.init_romaji_map();
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
    ime.init_romaji_map();
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
