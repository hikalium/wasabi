//! imedict: builds the WasabiOS IME dictionary from the manuscript.
//!
//! Every `*.md` and `*.typ` file under the given inputs is read. Words
//! that contain kanji, and katakana words, are picked out with a
//! morphological analyzer (lindera with the embedded IPADIC), keyed by
//! their reading in hiragana, and ordered by how often they appear.
//! Runs of nouns are also added as compound words. The `SEED` entries
//! always come first in their reading's candidate list.
//!
//! Writes `src/ime_dict.rs` in the wasabi repository and appends every
//! kanji and katakana seen to `third_party/unifont/chars_to_include.txt`
//! there; run `make -C third_party/unifont glyphs.txt` afterwards to
//! regenerate the glyphs. With no inputs, the dictionary holds just the
//! `SEED` entries.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;

/// Generates the IME dictionary and glyph list of the wasabi repository
/// from the words used in the manuscript.
#[derive(FromArgs)]
struct Args {
    /// path to the wasabi (code) repository to write into
    #[argh(option)]
    repo: PathBuf,
    /// manuscript files or directories (*.md and *.typ are read)
    #[argh(positional)]
    inputs: Vec<PathBuf>,
}

/// Hand-picked entries; their order of candidates is kept as is.
const SEED: &[(&str, &[&str])] = &[
    ("きょう", &["今日"]),
    ("てんき", &["天気"]),
    ("あめ", &["雨", "飴"]),
    ("わたし", &["私"]),
    ("なまえ", &["名前"]),
    ("なかの", &["中野"]),
    ("きしゃ", &["貴社", "記者", "汽車", "帰社"]),
];

const GLYPH_CHARS_PER_LINE: usize = 32;

// IPADIC token details:
// 品詞,細分類1,細分類2,細分類3,活用型,活用形,原形,読み,発音
const DETAIL_POS: usize = 0;
const DETAIL_POS_SUB: usize = 1;
const DETAIL_READING: usize = 7;

// BMP kanji (incl. 々); the OS font lookup is limited to the BMP.
fn is_kanji(c: char) -> bool {
    c == '々' || ('㐀'..='䶿').contains(&c) || ('一'..='鿿').contains(&c)
}

fn is_katakana(c: char) -> bool {
    c == 'ー' || ('ァ'..='ヺ').contains(&c)
}

fn is_hiragana_reading(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c == 'ー' || ('ぁ'..='ゖ').contains(&c))
}

fn to_hiragana(kana: &str) -> String {
    kana.chars()
        .map(|c| {
            if ('ァ'..='ヶ').contains(&c) {
                char::from_u32(c as u32 - 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

// Words worth a dictionary entry: anything with kanji in it, or a
// katakana word of two or more characters.
fn is_word(surface: &str) -> bool {
    surface.chars().any(is_kanji)
        || (surface.chars().count() >= 2 && surface.chars().all(is_katakana))
}

fn read_texts(inputs: &[PathBuf]) -> Result<Vec<String>> {
    fn collect(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        if path.is_dir() {
            let mut entries: Vec<_> = fs::read_dir(path)
                .with_context(|| format!("reading {}", path.display()))?
                .map(|e| e.map(|e| e.path()))
                .collect::<Result<_, _>>()?;
            entries.sort();
            for entry in entries {
                collect(&entry, files)?;
            }
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md") | Some("typ")
        ) {
            files.push(path.to_path_buf());
        }
        Ok(())
    }
    let mut files = Vec::new();
    for input in inputs {
        if input.is_file() {
            files.push(input.clone());
        } else {
            collect(input, &mut files)?;
        }
    }
    files
        .iter()
        .map(|f| {
            fs::read_to_string(f)
                .with_context(|| format!("reading {}", f.display()))
        })
        .collect()
}

type Counts = HashMap<(String, String), usize>;

fn add_word(counts: &mut Counts, reading: &str, surface: &str) {
    if is_hiragana_reading(reading) && is_word(surface) {
        *counts
            .entry((reading.to_string(), surface.to_string()))
            .or_default() += 1;
    }
}

// A run of two or more nouns counts as one compound word too.
fn flush_compound(counts: &mut Counts, nouns: &mut Vec<(String, String)>) {
    if nouns.len() >= 2 {
        let reading: String = nouns.iter().map(|(r, _)| r.as_str()).collect();
        let surface: String = nouns.iter().map(|(_, s)| s.as_str()).collect();
        add_word(counts, &reading, &surface);
    }
    nouns.clear();
}

fn count_words(texts: &[String]) -> Result<Counts> {
    let dictionary = load_dictionary("embedded://ipadic")?;
    let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
    let mut counts = Counts::new();
    let mut nouns = Vec::new();
    for text in texts {
        for line in text.lines() {
            let mut tokens = segmenter.segment(Cow::Borrowed(line))?;
            for token in tokens.iter_mut() {
                let surface = token.surface.to_string();
                let details = token.details();
                let Some(&kana) = details.get(DETAIL_READING) else {
                    flush_compound(&mut counts, &mut nouns);
                    continue;
                };
                if kana == "*" {
                    flush_compound(&mut counts, &mut nouns);
                    continue;
                }
                let reading = to_hiragana(kana);
                add_word(&mut counts, &reading, &surface);
                if details[DETAIL_POS] == "名詞"
                    && details[DETAIL_POS_SUB] != "数"
                {
                    nouns.push((reading, surface));
                } else {
                    flush_compound(&mut counts, &mut nouns);
                }
            }
            flush_compound(&mut counts, &mut nouns);
        }
    }
    Ok(counts)
}

fn build_dict(counts: &Counts) -> BTreeMap<String, Vec<String>> {
    let mut dict: BTreeMap<String, Vec<String>> = SEED
        .iter()
        .map(|(r, c)| {
            (r.to_string(), c.iter().map(|s| s.to_string()).collect())
        })
        .collect();
    let mut ordered: Vec<_> = counts.iter().collect();
    ordered.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for ((reading, surface), _) in ordered {
        if surface.contains(['"', '\\']) {
            continue;
        }
        let candidates = dict.entry(reading.clone()).or_default();
        if !candidates.contains(surface) {
            candidates.push(surface.clone());
        }
    }
    dict
}

fn render_dict(dict: &BTreeMap<String, Vec<String>>) -> String {
    let mut out = String::from(
        "// The IME dictionary: readings in hiragana and their candidates,\n\
         // most frequent first, sorted by reading.\n\
         // Generated from the manuscript by imedict\n\
         // (tools/imedict); do not edit by hand.\n\
         #[rustfmt::skip]\n\
         pub const DICT: &[(&str, &[&str])] = &[\n",
    );
    for (reading, candidates) in dict {
        let candidates: Vec<String> =
            candidates.iter().map(|c| format!("\"{c}\"")).collect();
        out += &format!("    (\"{reading}\", &[{}]),\n", candidates.join(", "));
    }
    out += "];\n";
    out
}

// Appends the kanji and katakana not yet in the glyph list.
fn extend_glyph_list(
    path: &Path,
    texts: &[String],
    dict: &BTreeMap<String, Vec<String>>,
) -> Result<usize> {
    let have: BTreeSet<char> = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?
        .chars()
        .collect();
    let seen: BTreeSet<char> = texts
        .iter()
        .flat_map(|t| t.chars())
        .chain(dict.values().flatten().flat_map(|s| s.chars()))
        .filter(|&c| (is_kanji(c) || is_katakana(c)) && !have.contains(&c))
        .collect();
    let need: Vec<char> = seen.into_iter().collect();
    let mut appended = String::new();
    for line in need.chunks(GLYPH_CHARS_PER_LINE) {
        appended.extend(line);
        appended.push('\n');
    }
    let mut content = fs::read_to_string(path)?;
    content += &appended;
    fs::write(path, content)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(need.len())
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let texts = read_texts(&args.inputs)?;
    let counts = if texts.is_empty() {
        Counts::new()
    } else {
        count_words(&texts)?
    };
    let dict = build_dict(&counts);
    let dict_rs = args.repo.join("src/ime_dict.rs");
    fs::write(&dict_rs, render_dict(&dict))
        .with_context(|| format!("writing {}", dict_rs.display()))?;
    let chars_txt = args.repo.join("third_party/unifont/chars_to_include.txt");
    let added = extend_glyph_list(&chars_txt, &texts, &dict)?;
    println!("{} readings written to {}", dict.len(), dict_rs.display());
    println!("{added} glyphs added to {}", chars_txt.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn katakana_reading_becomes_hiragana() {
        assert_eq!(to_hiragana("カンサイクーコー"), "かんさいくーこー");
    }

    #[test]
    fn words_need_kanji_or_two_katakana() {
        assert!(is_word("記憶"));
        assert!(is_word("動かす"));
        assert!(is_word("メモリ"));
        assert!(!is_word("ア"));
        assert!(!is_word("うごかす"));
        assert!(!is_word("USB"));
    }

    #[test]
    fn seed_candidates_come_first_then_by_frequency() {
        let mut counts = Counts::new();
        counts.insert(("あめ".into(), "編め".into()), 3);
        counts.insert(("きおく".into(), "記憶".into()), 2);
        counts.insert(("きおく".into(), "紀憶".into()), 5);
        let dict = build_dict(&counts);
        assert_eq!(dict["あめ"], ["雨", "飴", "編め"]);
        assert_eq!(dict["きおく"], ["紀憶", "記憶"]);
    }

    #[test]
    fn manuscript_words_are_segmented_with_readings() {
        let texts = vec!["仮想記憶を実装する。".to_string()];
        let counts = count_words(&texts).unwrap();
        assert_eq!(counts[&("かそう".to_string(), "仮想".to_string())], 1);
        assert_eq!(counts[&("きおく".to_string(), "記憶".to_string())], 1);
        assert_eq!(
            counts[&("かそうきおく".to_string(), "仮想記憶".to_string())],
            1
        );
        assert_eq!(counts[&("じっそう".to_string(), "実装".to_string())], 1);
    }
}
