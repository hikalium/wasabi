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

// The kanji candidates for one reading, most likely first.
type Candidates = &'static [&'static str];

// A dictionary word found at the start of a reading, and the rest of
// the reading (e.g. a particle) that follows it.
struct DictionaryMatch<'a> {
    candidates: Candidates,
    suffix: &'a str,
}

#[derive(Default)]
pub struct InputMethodEditor {
    pending_string: String,
    romaji_map2: BTreeMap<char, [&'static str; 5]>,
    romaji_map3: BTreeMap<(char, char), [&'static str; 5]>,
    // Reading (in kana) to its candidates. Keys and candidate lists
    // must be non-empty.
    dict: BTreeMap<&'static str, Candidates>,
}

impl InputMethodEditor {
    pub fn init_dict(&mut self) {
        self.dict.insert("きょう", &["今日"]);
        self.dict.insert("てんき", &["天気"]);
        self.dict.insert("あめ", &["雨"]);
    }
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
    /// The text typed so far that has not been committed yet.
    pub fn pending(&self) -> &str {
        &self.pending_string
    }
    /// Hands the pending text over to the caller for committing,
    /// leaving the IME empty.
    pub fn take_pending(&mut self) -> String {
        core::mem::take(&mut self.pending_string)
    }
    // Finds the longest dictionary word that the reading begins with.
    // A phrase is a word plus whatever attaches to it, so the reading
    // after the word is kept as the suffix.
    fn longest_match<'a>(
        &self,
        reading: &'a str,
    ) -> Option<DictionaryMatch<'a>> {
        self.dict
            .iter()
            .filter(|(word, _)| reading.starts_with(**word))
            .max_by_key(|(word, _)| word.len())
            .map(|(word, &candidates)| DictionaryMatch {
                candidates,
                // word.len() is a char boundary in reading, since
                // reading starts with word.
                suffix: &reading[word.len()..],
            })
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
            KeyEvent::Char(' ') => {
                // Space converts the word at the start of the pending
                // text and keeps the rest, or inserts a full-width space
                // when there is nothing to convert. A reading that does
                // not start with a dictionary word stays as is.
                if self.pending_string.is_empty() {
                    self.pending_string.push('　');
                } else if let Some(m) = self.longest_match(&self.pending_string)
                {
                    self.pending_string = [m.candidates[0], m.suffix].concat();
                }
                InputEditResult::UpdatePendingString(
                    self.pending_string.clone(),
                )
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

#[cfg(test)]
fn new_test_ime() -> InputMethodEditor {
    let mut ime = InputMethodEditor::default();
    ime.init_romaji_map();
    ime.init_dict();
    ime
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
fn today_conversion() {
    use alloc::string::ToString;
    let mut ime = new_test_ime();
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

#[test_case]
fn weather_conversion() {
    let mut ime = new_test_ime();
    assert_eq!(
        type_text(&mut ime, "tennki "),
        InputEditResult::UpdatePendingString("天気".to_string())
    );
}

#[test_case]
fn unmatched_space_is_consumed_without_insertion() {
    let mut ime = new_test_ime();
    assert_eq!(
        type_text(&mut ime, "sora "),
        InputEditResult::UpdatePendingString("そら".to_string())
    );
}

#[test_case]
fn empty_pending_space_inserts_fullwidth_space() {
    let mut ime = new_test_ime();
    assert_eq!(
        type_text(&mut ime, " "),
        InputEditResult::UpdatePendingString("　".to_string())
    );
}

#[test_case]
fn phrase_conversion_preserves_suffix() {
    for (typed, expected) in [
        ("kyouno ", "今日の"),
        ("tennkiha ", "天気は"),
        ("amedesu ", "雨です"),
    ] {
        let mut ime = new_test_ime();
        assert_eq!(
            type_text(&mut ime, typed),
            InputEditResult::UpdatePendingString(expected.to_string()),
            "{typed}"
        );
    }
}

#[test_case]
fn longest_dictionary_prefix_wins() {
    let mut ime = new_test_ime();
    // A shorter word that is also a prefix of the reading must lose.
    ime.dict.insert("きょ", &["短"]);
    assert_eq!(
        type_text(&mut ime, "kyouno "),
        InputEditResult::UpdatePendingString("今日の".to_string())
    );
}

#[test_case]
fn longest_prefix_is_not_grammatical_analysis() {
    // The suffix is not checked to be a particle or ending: あめりか
    // becomes 雨りか. This pins down the extent of the method.
    let mut ime = new_test_ime();
    assert_eq!(
        type_text(&mut ime, "amerika "),
        InputEditResult::UpdatePendingString("雨りか".to_string())
    );
}
