extern crate alloc;

use crate::keyboard::KeyEvent;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

#[derive(Default, PartialEq, Eq, Debug)]
pub enum InputEditResult {
    #[default]
    PassThrough,
    UpdatePendingString(String),
}

// The kanji candidates for one reading, most likely first.
pub type Candidates = &'static [&'static str];

// Readings (in kana) and their candidates. Readings and candidate
// lists must be non-empty.
pub type Dictionary = &'static [(&'static str, Candidates)];

// A dictionary word found at the start of a reading, and the rest of
// the reading (e.g. a particle) that follows it.
struct DictionaryMatch<'a> {
    candidates: Candidates,
    suffix: &'a str,
}

// A phrase whose candidates are being chosen from. While this exists,
// the pending text shows candidates[index].
struct Conversion {
    // The pending text as it was before converting, to go back to.
    reading: String,
    // What the pending text becomes for each candidate: the dictionary
    // candidates with the rest of the reading appended, then the
    // reading itself, then the reading in katakana.
    candidates: Vec<String>,
    index: usize,
}

fn to_katakana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ぁ'..='ゖ' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

#[derive(Default)]
pub struct InputMethodEditor {
    pending_string: String,
    conversion: Option<Conversion>,
    dict: Dictionary,
}

impl InputMethodEditor {
    // Points at the dictionary table, which the imedict tool in the
    // book repository generates from the manuscript. The table is used
    // in place: with thousands of words, copying it into a map would
    // make every Console take a second to create.
    pub fn init_dict(&mut self) {
        self.dict = crate::ime_dict::DICT;
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
        self.conversion = None;
        core::mem::take(&mut self.pending_string)
    }
    /// While a conversion is in progress: its candidates and which one
    /// is currently shown.
    pub fn candidates(&self) -> Option<(&[String], usize)> {
        self.conversion
            .as_ref()
            .map(|c| (c.candidates.as_slice(), c.index))
    }
    // Finds the longest dictionary word that the reading begins with.
    // A phrase is a word plus whatever attaches to it, so the reading
    // after the word is kept as the suffix. The table is sorted by
    // reading, so each prefix of the reading, longest first, is looked
    // up by binary search.
    fn longest_match<'a>(
        &self,
        reading: &'a str,
    ) -> Option<DictionaryMatch<'a>> {
        let ends = reading.char_indices().map(|(i, c)| i + c.len_utf8());
        ends.rev().find_map(|end| {
            let (word, suffix) = reading.split_at(end);
            let i = self
                .dict
                .binary_search_by(|(w, _)| str::cmp(w, word))
                .ok()?;
            Some(DictionaryMatch {
                candidates: self.dict[i].1,
                suffix,
            })
        })
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
                // Backspace. While choosing candidates it only cancels
                // the conversion, bringing the reading back untouched.
                if let Some(conversion) = self.conversion.take() {
                    self.pending_string = conversion.reading;
                    InputEditResult::UpdatePendingString(
                        self.pending_string.clone(),
                    )
                } else if self.pending_string.pop().is_some() {
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
                // when there is nothing to convert. The reading as typed
                // and its katakana close the candidate list, so a reading
                // without a dictionary word still has those two. Once
                // converted, each further Space shows the next candidate.
                if let Some(conversion) = self.conversion.as_mut() {
                    conversion.index =
                        (conversion.index + 1) % conversion.candidates.len();
                    self.pending_string =
                        conversion.candidates[conversion.index].clone();
                } else if self.pending_string.is_empty() {
                    self.pending_string.push('　');
                } else {
                    let reading = self.pending_string.clone();
                    let mut candidates: Vec<String> = self
                        .longest_match(&reading)
                        .map(|m| {
                            m.candidates
                                .iter()
                                .map(|c| [c, m.suffix].concat())
                                .collect()
                        })
                        .unwrap_or_default();
                    for extra in [reading.clone(), to_katakana(&reading)] {
                        if !candidates.contains(&extra) {
                            candidates.push(extra);
                        }
                    }
                    self.pending_string = candidates[0].clone();
                    self.conversion = Some(Conversion {
                        reading,
                        candidates,
                        index: 0,
                    });
                }
                InputEditResult::UpdatePendingString(
                    self.pending_string.clone(),
                )
            }
            KeyEvent::Char(prev1) => {
                // Typing on keeps the candidate shown and appends to it.
                self.conversion = None;
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

#[cfg(test)]
fn new_test_ime() -> InputMethodEditor {
    let mut ime = InputMethodEditor::default();
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
    // Without a dictionary nothing can match.
    let mut ime = InputMethodEditor::default();
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
    let dict: Dictionary = &[("きょ", &["短"]), ("きょう", &["今日"])];
    ime.dict = dict;
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

#[test_case]
fn rain_candidates_cycle() {
    let mut ime = new_test_ime();
    assert_eq!(
        type_text(&mut ime, "ame "),
        InputEditResult::UpdatePendingString("雨".to_string())
    );
    // After the dictionary candidates come the reading and its
    // katakana; past the last one, Space wraps around to the first.
    for expected in ["飴", "あめ", "アメ", "雨"] {
        assert_eq!(
            type_text(&mut ime, " "),
            InputEditResult::UpdatePendingString(expected.to_string())
        );
    }
}

#[test_case]
fn candidate_cycle_preserves_suffix() {
    let mut ime = new_test_ime();
    assert_eq!(
        type_text(&mut ime, "amedesu  "),
        InputEditResult::UpdatePendingString("飴です".to_string())
    );
}

#[test_case]
fn backspace_cancels_conversion_before_deleting_reading() {
    let mut ime = new_test_ime();
    type_text(&mut ime, "amedesu ");
    assert_eq!(
        type_text(&mut ime, "\x08"),
        InputEditResult::UpdatePendingString("あめです".to_string())
    );
    assert_eq!(
        type_text(&mut ime, "\x08"),
        InputEditResult::UpdatePendingString("あめで".to_string())
    );
}

#[test_case]
fn commit_clears_conversion_state() {
    let mut ime = new_test_ime();
    type_text(&mut ime, "ame ");
    assert_eq!(ime.take_pending(), "雨");
    assert!(ime.conversion.is_none());
    // Space after the commit starts afresh instead of cycling.
    assert_eq!(
        type_text(&mut ime, " "),
        InputEditResult::UpdatePendingString("　".to_string())
    );
}

#[test_case]
fn typing_after_conversion_keeps_candidate_and_appends() {
    let mut ime = new_test_ime();
    assert_eq!(
        type_text(&mut ime, "ame ga"),
        InputEditResult::UpdatePendingString("雨が".to_string())
    );
    assert!(ime.conversion.is_none());
}

#[test_case]
fn homophone_candidates_cycle_in_dictionary_order() {
    let mut ime = new_test_ime();
    assert_eq!(
        type_text(&mut ime, "kisya "),
        InputEditResult::UpdatePendingString("貴社".to_string())
    );
    for expected in ["記者", "汽車", "帰社", "きしゃ", "キシャ", "貴社"]
    {
        assert_eq!(
            type_text(&mut ime, " "),
            InputEditResult::UpdatePendingString(expected.to_string())
        );
    }
}

#[test_case]
fn comma_and_period_become_japanese_punctuation() {
    use crate::font::get_glyph_width;
    let mut ime = new_test_ime();
    assert_eq!(
        type_text(&mut ime, "hai,sou."),
        InputEditResult::UpdatePendingString("はい、そう。".to_string())
    );
    assert_eq!(get_glyph_width('、'), 16);
    assert_eq!(get_glyph_width('。'), 16);
}

#[test_case]
fn dictionary_table_is_sorted_for_binary_search() {
    let dict = crate::ime_dict::DICT;
    assert!(dict.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert!(dict.iter().all(|(r, c)| !r.is_empty() && !c.is_empty()));
}

#[test_case]
fn jy_spells_the_ja_row() {
    let mut ime = new_test_ime();
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
        let mut ime = new_test_ime();
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
        let mut ime = new_test_ime();
        assert_eq!(
            type_text(&mut ime, typed),
            InputEditResult::UpdatePendingString(expected.to_string()),
            "{typed}"
        );
    }
}

#[test_case]
fn reading_and_katakana_close_the_candidates() {
    // Without a dictionary word the reading itself comes first, then
    // its katakana.
    let mut ime = InputMethodEditor::default();
    for (typed, expected) in [("sora ", "そら"), (" ", "ソラ"), (" ", "そら")]
    {
        assert_eq!(
            type_text(&mut ime, typed),
            InputEditResult::UpdatePendingString(expected.to_string())
        );
    }
    // With one, they follow the dictionary candidates, and the katakana
    // covers the whole pending text.
    let mut ime = new_test_ime();
    for (typed, expected) in [
        ("kyouno ", "今日の"),
        (" ", "きょうの"),
        (" ", "キョウノ"),
        (" ", "今日の"),
    ] {
        assert_eq!(
            type_text(&mut ime, typed),
            InputEditResult::UpdatePendingString(expected.to_string())
        );
    }
}
