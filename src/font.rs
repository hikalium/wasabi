use core::cmp::Ordering;
use core::ops::Range;

const JP_FONT_SOURCE: &str = include_str!("../third_party/unifont/glyphs.txt");

// glyphs.txt has 16 lines per glyph, and every line is
// "glyph_XXXX.txt:" + 16 pixels + '\n' = 32 bytes, so each glyph is a
// 512-byte record. The records are sorted by code point, so a glyph is
// found by binary search over the file itself: nothing is built up
// front, however many glyphs there are.
const JP_GLYPH_LINE_LEN: usize = 32;
const JP_GLYPH_RECORD_LEN: usize = 16 * JP_GLYPH_LINE_LEN;
// The hex code point in the header, and the 16 pixels of a line.
const JP_GLYPH_CODE: Range<usize> = 6..10;
const JP_GLYPH_PIXELS: Range<usize> = 15..31;

fn jp_glyph_record(i: usize) -> Option<&'static [u8]> {
    JP_FONT_SOURCE
        .as_bytes()
        .get(i * JP_GLYPH_RECORD_LEN..(i + 1) * JP_GLYPH_RECORD_LEN)
}

fn jp_glyph_code(record: &[u8]) -> Option<u32> {
    let hex = core::str::from_utf8(record.get(JP_GLYPH_CODE)?).ok()?;
    u32::from_str_radix(hex, 16).ok()
}

// The record of c's 16x16 glyph, if the font has one.
fn find_glyph_16x16(c: char) -> Option<&'static [u8]> {
    let (mut lo, mut hi) = (0, JP_FONT_SOURCE.len() / JP_GLYPH_RECORD_LEN);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let record = jp_glyph_record(mid)?;
        match jp_glyph_code(record)?.cmp(&(c as u32)) {
            Ordering::Less => lo = mid + 1,
            Ordering::Greater => hi = mid,
            Ordering::Equal => return Some(record),
        }
    }
    None
}

pub fn get_glyph_width(c: char) -> i64 {
    if lookup_font_8x16(c).is_some() {
        8
    } else if find_glyph_16x16(c).is_some() {
        16
    } else {
        0
    }
}

#[test_case]
fn glyphs_txt_is_sorted_fixed_size_records() {
    assert_eq!(JP_FONT_SOURCE.len() % JP_GLYPH_RECORD_LEN, 0);
    let mut prev = None;
    for i in 0..JP_FONT_SOURCE.len() / JP_GLYPH_RECORD_LEN {
        let record = jp_glyph_record(i).unwrap();
        let code = jp_glyph_code(record).unwrap();
        assert!(prev < Some(code), "glyph {code:04X} is out of order");
        prev = Some(code);
        for line in record.chunks(JP_GLYPH_LINE_LEN) {
            assert_eq!(&line[..6], b"glyph_", "glyph {code:04X}");
            assert_eq!(&line[10..15], b".txt:", "glyph {code:04X}");
            assert_eq!(line[31], b'\n', "glyph {code:04X}");
        }
    }
}

#[test_case]
fn return_8_for_alphabet() {
    assert_eq!(get_glyph_width('A'), 8);
}

#[test_case]
fn return_16_for_日() {
    assert_eq!(get_glyph_width('日'), 16);
}

pub fn lookup_font_16x16(c: char) -> Option<[[char; 16]; 16]> {
    let record = find_glyph_16x16(c)?;
    let mut font = [[' '; 16]; 16];
    for (y, line) in record.chunks(JP_GLYPH_LINE_LEN).enumerate() {
        for (x, &pixel) in line.get(JP_GLYPH_PIXELS)?.iter().enumerate() {
            font[y][x] = pixel as char;
        }
    }
    Some(font)
}

pub fn lookup_font_8x16(c: char) -> Option<[[char; 8]; 16]> {
    const FONT_SOURCE: &str = include_str!("./font.txt");
    static mut FONT_CACHE: Option<[[[char; 8]; 16]; 256]> = None;
    if let Ok(c) = u8::try_from(c) {
        let font = unsafe {
            FONT_CACHE.get_or_insert_with(|| {
                let mut font = [[['*'; 8]; 16]; 256];
                let mut fi = FONT_SOURCE.split('\n');
                while let Some(line) = fi.next() {
                    if let Some(line) = line.strip_prefix("0x") {
                        if let Ok(idx) = u8::from_str_radix(line, 16) {
                            let mut glyph = [['*'; 8]; 16];
                            for (y, line) in fi.clone().take(16).enumerate() {
                                for (x, c) in line.chars().enumerate() {
                                    if let Some(e) = glyph[y].get_mut(x) {
                                        *e = c;
                                    }
                                }
                            }
                            font[idx as usize] = glyph;
                        }
                    }
                }
                font
            })
        };
        Some(font[c as usize])
    } else {
        None
    }
}

#[test_case]
fn conversion_glyphs_are_available() {
    for c in "今日天気雨飴私名前中野貴社記者汽車帰".chars() {
        assert_eq!(get_glyph_width(c), 16, "{c}");
    }
}
