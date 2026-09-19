//! Readings of Japanese words from a jawiktionary pages-articles dump.
//!
//! The dump (`jawiktionary-latest-pages-articles.xml.bz2` from
//! <https://dumps.wikimedia.org/jawiktionary/latest/>) is read page by
//! page, straight from the compressed file. For every article whose
//! title is a Japanese word containing kanji and that has a Japanese
//! section (`=={{L|ja}}==`, `=={{ja}}==` or `==日本語==`), the readings
//! are taken from `{{kana-DEFAULTSORT|よみ}}`, from the positional
//! arguments of the headword templates in that section (`{{ja-noun|よみ}}`,
//! `{{ja-verb|よみ}}`, `{{ja-pron|よみ|...}}`, `{{ja-wagokanji|よみ|よみ}}`,
//! ...), and from the headword line `'''語'''（よみ）`. Only readings
//! written entirely in hiragana are kept.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use bzip2::read::MultiBzDecoder;

use crate::is_hiragana_reading;
use crate::is_kanji;
use crate::is_katakana;

// Templates whose positional arguments are readings of the headword.
const READING_TEMPLATES: &[&str] = &[
    "kana-DEFAULTSORT",
    "ja-noun",
    "ja-verb",
    "ja-adj",
    "ja-adv",
    "ja-pron",
    "ja-wagokanji",
    "ja-proper",
    "ja-propn",
    "ja-interj",
    "ja-conj",
    "ja-phrase",
    "ja-pos",
];

const MAX_TITLE_CHARS: usize = 12;

/// All (reading, word) pairs found in the dump at `path` (`.xml` or
/// `.xml.bz2`), each pair once.
pub fn words(path: &Path) -> Result<Vec<(String, String)>> {
    let file = File::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let reader: Box<dyn BufRead> =
        if path.extension().and_then(|e| e.to_str()) == Some("bz2") {
            Box::new(BufReader::new(MultiBzDecoder::new(file)))
        } else {
            Box::new(BufReader::new(file))
        };
    let mut pairs = BTreeSet::new();
    let mut title = String::new();
    let mut is_article = false;
    // The wikitext of the page being read, once inside <text>.
    let mut text: Option<String> = None;
    for line in reader.lines() {
        let line = line?;
        if let Some(body) = text.as_mut() {
            if let Some(end) = line.find("</text>") {
                body.push_str(&line[..end]);
                let body = text.take().unwrap_or_default();
                if is_article {
                    collect(&title, &unescape(&body), &mut pairs);
                }
            } else {
                body.push_str(&line);
                body.push('\n');
            }
            continue;
        }
        let tag = line.trim();
        if let Some(rest) = tag.strip_prefix("<title>") {
            title = unescape(rest.trim_end_matches("</title>"));
        } else if tag.starts_with("<ns>") {
            is_article = tag == "<ns>0</ns>";
        } else if tag.starts_with("<text") && !tag.ends_with("/>") {
            let Some(start) = tag.find('>') else { continue };
            let body = &tag[start + 1..];
            if let Some(end) = body.find("</text>") {
                if is_article {
                    collect(&title, &unescape(&body[..end]), &mut pairs);
                }
            } else {
                text = Some(format!("{body}\n"));
            }
        }
    }
    Ok(pairs.into_iter().collect())
}

fn collect(title: &str, text: &str, pairs: &mut BTreeSet<(String, String)>) {
    if !is_japanese_word(title) {
        return;
    }
    for reading in readings(text) {
        if reading != title {
            pairs.insert((reading, title.to_string()));
        }
    }
}

// A word worth an entry: kanji with, at most, kana around them, and not
// unreasonably long (proverbs and the like are not for an IME).
fn is_japanese_word(title: &str) -> bool {
    title.chars().any(is_kanji)
        && title.chars().count() <= MAX_TITLE_CHARS
        && title.chars().all(|c| {
            is_kanji(c) || is_katakana(c) || ('ぁ'..='ゖ').contains(&c)
        })
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&amp;", "&")
}

/// The hiragana readings given for the word in the page's wikitext.
pub fn readings(text: &str) -> Vec<String> {
    let mut readings: Vec<String> = Vec::new();
    let Some(section) = japanese_section(text) else {
        return readings;
    };
    let mut add = |candidate: &str| {
        let candidate = candidate.trim();
        if is_hiragana_reading(candidate)
            && !readings.iter().any(|r| r == candidate)
        {
            readings.push(candidate.to_string());
        }
    };
    for args in templates(text, "kana-DEFAULTSORT") {
        args.iter().for_each(|a| add(a));
    }
    for name in READING_TEMPLATES {
        for args in templates(&section, name) {
            for arg in args {
                match arg.split_once('=') {
                    None => add(&arg),
                    Some(("hira" | "kana", value)) => add(value),
                    Some(_) => {}
                }
            }
        }
    }
    // '''語'''（よみ、よみ）: alternatives are separated by 、 or ・; a
    // reading may be written in parts, "ちゅうおう あめりか" or
    // "かん-じ", which are joined.
    for line in section.lines().filter(|l| l.starts_with("'''")) {
        if let Some((_, rest)) = line.split_once('（') {
            if let Some((inner, _)) = rest.split_once('）') {
                for alternative in
                    strip_links(inner).split(['、', '・', '，', ','])
                {
                    let joined: String = alternative
                        .chars()
                        .filter(|c| {
                            !c.is_whitespace() && *c != '-' && *c != '‐'
                        })
                        .collect();
                    add(&joined);
                }
            }
        }
    }
    readings
}

// The lines under the Japanese level-2 heading, up to the next one.
fn japanese_section(text: &str) -> Option<String> {
    let mut section = String::new();
    let mut inside = false;
    for line in text.lines() {
        let heading = line.trim();
        if heading.starts_with("==")
            && !heading.starts_with("===")
            && heading.ends_with("==")
        {
            let name = heading.trim_matches('=').trim();
            inside = matches!(name, "{{L|ja}}" | "{{ja}}" | "日本語");
            continue;
        }
        if inside {
            section.push_str(line);
            section.push('\n');
        }
    }
    (!section.is_empty()).then_some(section)
}

// The argument lists of every `{{name|...}}` in text, links unwrapped.
fn templates(text: &str, name: &str) -> Vec<Vec<String>> {
    let opener = format!("{{{{{name}|");
    text.match_indices(&opener)
        .filter_map(|(start, _)| {
            let body = &text[start + opener.len()..];
            let end = body.find("}}")?;
            Some(
                strip_links(&body[..end])
                    .split('|')
                    .map(|a| a.trim().to_string())
                    .collect(),
            )
        })
        .collect()
}

// `[[target|shown]]` and `[[target]]` become `target`.
fn strip_links(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("[[") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else {
            rest = after;
            break;
        };
        let target = after[..end].split('|').next().unwrap_or("");
        out.push_str(target);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_come_from_sort_key_templates_and_headword() {
        let text = "{{kana-DEFAULTSORT|かんじ}}\n\
                    =={{L|ja}}==\n\
                    ==={{verb}}===\n\
                    '''[[感]]じ'''（[[かんじ|カン-じ]]）\n\
                    ==={{noun}}===\n\
                    {{ja-noun|[[かんじ]]}}\n\
                    ===={{pron}}====\n\
                    {{ja-pron|かんじ|acc=0}}\n";
        assert_eq!(readings(text), ["かんじ"]);
    }

    #[test]
    fn headword_reading_parts_are_joined_and_alternatives_split() {
        let text = "=={{ja}}==\n==={{name}}===\n\
                    '''[[中央]][[アメリカ]]'''（ちゅうおう あめりか）\n";
        assert_eq!(readings(text), ["ちゅうおうあめりか"]);
        let text = "=={{ja}}==\n'''今日'''（きょう、こんにち・こんじつ）\n";
        assert_eq!(readings(text), ["きょう", "こんにち", "こんじつ"]);
    }

    #[test]
    fn wagokanji_gives_every_reading() {
        let text = "{{kana-DEFAULTSORT|よい}}\n=={{L|ja}}==\n\
                    ==={{wago}}===\n{{ja-wagokanji|よい|いい}}\n";
        assert_eq!(readings(text), ["よい", "いい"]);
    }

    #[test]
    fn only_the_japanese_section_counts() {
        let text = "=={{L|zh}}==\n{{ja-noun|ほげ}}\n'''感じ'''（ふが）\n\
                    =={{L|ja}}==\n{{ja-noun|かんじ}}\n\
                    =={{L|ko}}==\n{{ja-noun|ぴよ}}\n";
        assert_eq!(readings(text), ["かんじ"]);
        assert!(readings("=={{L|en}}==\n{{en-noun}}\n").is_empty());
    }

    #[test]
    fn the_sort_key_counts_only_with_a_japanese_section() {
        let text = "{{kana-DEFAULTSORT|かんじ}}\n=={{L|ja}}==\n'''感じ'''\n";
        assert_eq!(readings(text), ["かんじ"]);
        let text = "{{kana-DEFAULTSORT|かんじ}}\n=={{L|en}}==\n";
        assert!(readings(text).is_empty());
    }

    #[test]
    fn titles_must_be_japanese_words_with_kanji() {
        assert!(is_japanese_word("感じる"));
        assert!(is_japanese_word("良い"));
        assert!(!is_japanese_word("かんじ"));
        assert!(!is_japanese_word("感じ (小説)"));
        assert!(!is_japanese_word("ABC"));
    }

    #[test]
    fn links_are_unwrapped() {
        assert_eq!(
            strip_links("[[感]]じ（[[かんじ|カン-じ]]）"),
            "感じ（かんじ）"
        );
    }
}
