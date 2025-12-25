extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

const JP_FONT_SOURCE: &str = include_str!("../third_party/unifont/glyphs.txt");

static mut JP_FONT_CACHE: Option<BTreeMap<String, [[char; 16]; 16]>> = None;

pub fn get_glyph_width(c: char) -> i64 {
    if lookup_font_8x16(c).is_some() {
        8
    } else if lookup_font_16x16(c).is_some() {
        16
    } else {
        0
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
    let font_cache = unsafe {
        JP_FONT_CACHE.get_or_insert_with(|| {
            let mut font = [[' '; 16]; 16];
            let mut font_row_idx = 0;
            let mut fonts: BTreeMap<String, [[char; 16]; 16]> = BTreeMap::new();
            let mut prev_header = None;
            for row in JP_FONT_SOURCE.split('\n') {
                if let Some((header, bmprow)) = &row.split_once(':') {
                    if Some(header.to_string()) != prev_header {
                        if prev_header.is_some() {
                            fonts.insert(prev_header.unwrap_or_default(), font);
                            font_row_idx = 0;
                        }
                        prev_header = Some(header.to_string());
                    }
                    let mut font_row = [' '; 16];
                    let bmprow: Vec<char> = bmprow.chars().collect();
                    font_row.copy_from_slice(&bmprow[0..16]);
                    if font_row_idx < font.len() {
                        font[font_row_idx] = font_row;
                    }
                    font_row_idx += 1;
                }
            }
            if prev_header.is_some() {
                fonts.insert(prev_header.unwrap_or_default(), font);
            }
            fonts
        })
    };
    font_cache
        .get(&format!("glyph_{:04X}.txt", c as u16))
        .copied()
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
