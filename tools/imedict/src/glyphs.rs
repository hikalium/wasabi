//! glyphs.txt for the OS, cut out of the GNU Unifont Japanese BDF.
//!
//! The OS keeps its 16x16 font as `third_party/unifont/glyphs.txt`: for
//! every character listed in `chars_to_include.txt`, 16 lines of
//! `glyph_XXXX.txt:` followed by 16 pixels (`#` or `.`), records sorted
//! by code point. The BDF is read once, so several thousand glyphs take
//! well under a second, where the old Makefile grepped the whole font
//! file once per glyph.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;

/// Where the BDF comes from when it is not on disk yet.
pub const UNIFONT_URL: &str = concat!(
    "https://unifoundry.com/pub/unifont/unifont-16.0.04/",
    "font-builds/unifont_jp-16.0.04.bdf.gz"
);

const ROWS: usize = 16;

/// Writes `glyphs_txt` for the characters listed in `chars_txt`, taking
/// the bitmaps from the BDF at `bdf` (downloaded there first if
/// missing). Returns the glyphs written and the characters the font
/// does not have.
pub fn write_glyphs(
    bdf: &Path,
    chars_txt: &Path,
    glyphs_txt: &Path,
) -> Result<(usize, Vec<char>)> {
    let wanted: BTreeSet<u32> = fs::read_to_string(chars_txt)
        .with_context(|| format!("reading {}", chars_txt.display()))?
        .chars()
        // The font format holds the BMP only.
        .filter(|&c| c != '\n' && c != '\r' && (c as u32) <= 0xFFFF)
        .map(|c| c as u32)
        .collect();
    let font = load_bdf(bdf)?;
    let bitmaps = parse_bdf(&font, &wanted);
    let mut out = String::new();
    for (&code, rows) in &bitmaps {
        for row in rows {
            out += &format!("glyph_{code:04X}.txt:{}\n", render_row(*row));
        }
    }
    fs::write(glyphs_txt, out)
        .with_context(|| format!("writing {}", glyphs_txt.display()))?;
    let missing = wanted
        .iter()
        .filter(|c| !bitmaps.contains_key(c))
        .filter_map(|&c| char::from_u32(c))
        .collect();
    Ok((bitmaps.len(), missing))
}

// The BDF text, fetched and unpacked into `path` when it is not there.
fn load_bdf(path: &Path) -> Result<String> {
    if !path.exists() {
        eprintln!("downloading {UNIFONT_URL}");
        let gz = ureq::get(UNIFONT_URL)
            .call()
            .with_context(|| format!("downloading {UNIFONT_URL}"))?
            .body_mut()
            .read_to_vec()?;
        let mut bdf = String::new();
        flate2::read::GzDecoder::new(&gz[..]).read_to_string(&mut bdf)?;
        fs::write(path, &bdf)
            .with_context(|| format!("writing {}", path.display()))?;
        return Ok(bdf);
    }
    let bytes = fs::read(path)
        .with_context(|| format!("reading {}", path.display()))?;
    if path.extension().and_then(|e| e.to_str()) == Some("gz") {
        let mut bdf = String::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut bdf)?;
        return Ok(bdf);
    }
    String::from_utf8(bytes)
        .with_context(|| format!("{} is not UTF-8", path.display()))
}

// The 16 bitmap rows of every wanted glyph, by code point. A BDF glyph
// is `STARTCHAR`, `ENCODING <decimal>`, metrics, `BITMAP`, one hex row
// per line, `ENDCHAR`.
fn parse_bdf(font: &str, wanted: &BTreeSet<u32>) -> BTreeMap<u32, [u16; ROWS]> {
    let mut bitmaps = BTreeMap::new();
    let mut code: Option<u32> = None;
    let mut rows: Vec<u16> = Vec::new();
    let mut in_bitmap = false;
    for line in font.lines() {
        let line = line.trim_end();
        if let Some(encoding) = line.strip_prefix("ENCODING ") {
            code = encoding.trim().parse().ok().filter(|c| wanted.contains(c));
            rows.clear();
            in_bitmap = false;
        } else if line.starts_with("BITMAP") {
            in_bitmap = true;
        } else if line == "ENDCHAR" {
            if let Some(c) = code.take() {
                bitmaps.insert(c, fit_rows(&rows));
            }
            in_bitmap = false;
        } else if in_bitmap && code.is_some() {
            // A half-width glyph has 2 hex digits per row, a full-width
            // one 4; either way the value is the row, right-aligned.
            if let Ok(row) = u16::from_str_radix(line, 16) {
                rows.push(row);
            }
        }
    }
    bitmaps
}

// The last 16 rows, padded with blank rows at the top if there are fewer.
fn fit_rows(rows: &[u16]) -> [u16; ROWS] {
    let mut fitted = [0u16; ROWS];
    let skip = rows.len().saturating_sub(ROWS);
    let start = ROWS.saturating_sub(rows.len());
    for (dst, src) in fitted[start..].iter_mut().zip(&rows[skip..]) {
        *dst = *src;
    }
    fitted
}

fn render_row(row: u16) -> String {
    (0..16)
        .rev()
        .map(|bit| if row >> bit & 1 == 1 { '#' } else { '.' })
        .collect()
}

pub fn check_bdf_name(path: &Path) -> Result<()> {
    if path.extension().is_none() {
        bail!("{} should end in .bdf or .bdf.gz", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT: &str = "STARTFONT 2.1\n\
        STARTCHAR U+0041\nENCODING 65\nBBX 8 16 0 -2\nBITMAP \n\
        00\n00\n18\n24\n42\n42\n7E\n42\n\
        42\n42\n42\n00\n00\n00\n00\n00\nENDCHAR\n\
        STARTCHAR U+3042\nENCODING 12354\nBBX 16 16 0 -2\nBITMAP \n\
        0000\n0100\n0100\n0160\n0F80\n0200\n0220\n03F8\n\
        0644\n0A52\n1252\n2242\n2244\n1C38\n0060\n0000\nENDCHAR\n\
        ENDFONT\n";

    #[test]
    fn rows_render_msb_first_with_hash_for_set_bits() {
        assert_eq!(render_row(0x0100), ".......#........");
        assert_eq!(render_row(0xF00F), "####........####");
    }

    #[test]
    fn only_wanted_glyphs_are_parsed_and_half_width_is_right_aligned() {
        let wanted: BTreeSet<u32> = [0x41, 0x3042].into_iter().collect();
        let bitmaps = parse_bdf(FONT, &wanted);
        assert_eq!(bitmaps.len(), 2);
        assert_eq!(render_row(bitmaps[&0x3042][3]), ".......#.##.....");
        assert_eq!(render_row(bitmaps[&0x41][2]), "...........##...");
        assert!(parse_bdf(FONT, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn short_bitmaps_are_padded_at_the_top() {
        assert_eq!(fit_rows(&[1, 2])[14..], [1, 2]);
        assert_eq!(fit_rows(&[0; 20])[..], [0; 16]);
    }
}
