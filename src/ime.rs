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
    /// Replace the pending (pre-conversion) string. Used by callers to
    /// keep the IME in sync after they reset or reload the input line.
    pub fn set_pending(&mut self, s: String) {
        self.pending_string = s;
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
            'f' => Some(["ふぁ", "ふぃ", "ふ", "ふぇ", "ふぉ"]),
            'v' => Some(["ゔぁ", "ゔぃ", "ゔ", "ゔぇ", "ゔぉ"]),
            'c' => Some(["か", "し", "く", "せ", "こ"]),
            'q' => Some(["くぁ", "くぃ", "く", "くぇ", "くぉ"]),
            // x or l + vowel is the small vowel; xtu/ltu are っ, and
            // xya/xyu/xyo (lya/lyu/lyo) are ゃゅょ.
            'x' => Some(["ぁ", "ぃ", "ぅ", "ぇ", "ぉ"]),
            'l' => Some(["ぁ", "ぃ", "ぅ", "ぇ", "ぉ"]),
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
            ('j', 'y') => Some(["じゃ", "じぃ", "じゅ", "じぇ", "じょ"]),
            ('z', 'y') => Some(["じゃ", "じぃ", "じゅ", "じぇ", "じょ"]),
            ('t', 's') => Some(["つぁ", "つぃ", "つ", "つぇ", "つぉ"]),
            ('t', 'h') => Some(["てゃ", "てぃ", "てゅ", "てぇ", "てょ"]),
            ('d', 'h') => Some(["でゃ", "でぃ", "でゅ", "でぇ", "でょ"]),
            ('w', 'h') => Some(["うぁ", "うぃ", "う", "うぇ", "うぉ"]),
            ('f', 'y') => Some(["ふゃ", "ふぃ", "ふゅ", "ふぇ", "ふょ"]),
            ('v', 'y') => Some(["ゔゃ", "ゔぃ", "ゔゅ", "ゔぇ", "ゔょ"]),
            ('k', 'w') => Some(["くぁ", "くぃ", "くぅ", "くぇ", "くぉ"]),
            ('g', 'w') => Some(["ぐぁ", "ぐぃ", "ぐぅ", "ぐぇ", "ぐぉ"]),
            ('x', 'y') => Some(["ゃ", "ぃ", "ゅ", "ぇ", "ょ"]),
            ('x', 't') => Some(["", "", "っ", "", ""]),
            ('l', 'y') => Some(["ゃ", "ぃ", "ゅ", "ぇ", "ょ"]),
            ('l', 't') => Some(["", "", "っ", "", ""]),
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
                    } else if ',' == prev1 {
                        String::from('、')
                    } else if '.' == prev1 {
                        String::from('。')
                    } else if let (Some('n'), 'n') = (prev2, prev1) {
                        self.pending_string.pop();
                        String::from('ん')
                    } else if prev2 == Some('n')
                        && Self::is_consonant(prev1)
                        && prev1 != 'y'
                    {
                        // n before another consonant is ん (before y it
                        // is the start of にゃ/にゅ/にょ).
                        self.pending_string.pop();
                        String::from('ん') + &String::from(prev1)
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

// Types s one key at a time and returns the result of the last key.
#[cfg(test)]
fn type_text(ime: &mut InputMethodEditor, s: &str) -> InputEditResult {
    s.chars()
        .map(|c| ime.send_key_down(KeyEvent::Char(c)))
        .last()
        .unwrap_or_default()
}

#[test_case]
fn jy_spells_the_ja_row() {
    let mut ime = InputMethodEditor::default();
    assert_eq!(
        type_text(&mut ime, "jyuu"),
        InputEditResult::UpdatePendingString("じゅう".to_string())
    );
    assert_eq!(
        type_text(&mut ime, "jya"),
        InputEditResult::UpdatePendingString("じゅうじゃ".to_string())
    );
}

#[test_case]
fn extended_romaji_rows() {
    use crate::font::get_glyph_width;
    for (typed, expected) in [
        ("fu", "ふ"),
        ("fa", "ふぁ"),
        ("fyu", "ふゅ"),
        ("va", "ゔぁ"),
        ("tsu", "つ"),
        ("thi", "てぃ"),
        ("dhi", "でぃ"),
        ("whi", "うぃ"),
        ("zyu", "じゅ"),
        ("ca", "か"),
        ("qa", "くぁ"),
        ("kwa", "くぁ"),
        ("xa", "ぁ"),
        ("xtu", "っ"),
        ("xyo", "ょ"),
        ("la", "ぁ"),
        ("ltu", "っ"),
        ("lyu", "ゅ"),
    ] {
        let mut ime = InputMethodEditor::default();
        assert_eq!(
            type_text(&mut ime, typed),
            InputEditResult::UpdatePendingString(expected.to_string()),
            "{typed}"
        );
    }
    for c in "ぁぅぉゔ".chars() {
        assert_eq!(get_glyph_width(c), 16, "{c}");
    }
}

#[test_case]
fn n_before_a_consonant_is_nn() {
    for (typed, expected) in [
        ("tenki", "てんき"),
        ("kanji", "かんじ"),
        ("nya", "にゃ"),
        ("nn", "ん"),
        ("na", "な"),
    ] {
        let mut ime = InputMethodEditor::default();
        assert_eq!(
            type_text(&mut ime, typed),
            InputEditResult::UpdatePendingString(expected.to_string()),
            "{typed}"
        );
    }
}

#[test_case]
fn comma_and_period_become_japanese_punctuation() {
    use crate::font::get_glyph_width;
    let mut ime = InputMethodEditor::default();
    assert_eq!(
        type_text(&mut ime, "hai,sou."),
        InputEditResult::UpdatePendingString("はい、そう。".to_string())
    );
    assert_eq!(get_glyph_width('、'), 16);
    assert_eq!(get_glyph_width('。'), 16);
}
