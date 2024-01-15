use crate::error::Error;
use crate::error::Result;
use crate::font::BITMAP_FONT;

/// Draws string in one line. New lines are ignored.
pub fn draw_string_3x(color: u32, x: i64, y: i64, s: &str) -> Result<()> {
    let mut pos = 0;
    for c in s.chars() {
        draw_char_3x(color, x + pos, y, c)?;
        pos += 24;
    }
    Ok(())
}

/// Draws string in one line. New lines are ignored.
pub fn draw_string_2x(color: u32, x: i64, y: i64, s: &str) -> Result<()> {
    let mut pos = 0;
    for c in s.chars() {
        draw_char_2x(color, x + pos, y, c)?;
        pos += 16;
    }
    Ok(())
}

pub fn draw_string_1p5x(color: u32, x: i64, y: i64, s: &str) -> Result<()> {
    let mut pos = 0;
    for c in s.chars() {
        draw_char_1p5x(color, x + pos, y, c)?;
        pos += 12;
    }
    Ok(())
}

/// Draws string in one line. New lines are ignored.
pub fn draw_string(color: u32, x: i64, y: i64, s: &str) -> Result<()> {
    let mut pos = 0;
    for c in s.chars() {
        draw_char(color, x + pos, y, c)?;
        pos += 8;
    }
    Ok(())
}

pub fn draw_string_with_underline(color: u32, x: i64, y: i64, s: &str) -> Result<()> {
    let mut pos = 0;
    for c in s.chars() {
        draw_char(color, x + pos, y, c)?;
        pos += 8;
    }
    draw_line(color, x, y + 16, x + pos, y + 16)?;
    Ok(())
}

// 1x
// ooo
// o1o
// ooo
//
// 3x
// ooooooooo
// ooooooooo
// ooooooooo
// ooo111ooo
// ooo111ooo
// ooo111ooo
// ooooooooo
// ooooooooo
// ooooooooo
//
// 半分
// oooo
// o11o
// o11o
// oooo
pub fn draw_char_1p5x(color: u32, px: i64, py: i64, c: char) -> Result<()> {
    // size (1x): 8 * 16
    // size (1.5x): 12 * 24
    // size (2x): 16 * 32
    // size (3x): 24 * 48
    let font_data = BITMAP_FONT[c as usize];
    let mut font_3x = [[false; 24]; 48];

    for (y, font_3x_row_bitmap) in font_3x.iter_mut().enumerate() {
        for (x, font_3x_pixel) in font_3x_row_bitmap.iter_mut().enumerate() {
            let original_x = x / 3;
            let original_y = y / 3;
            if (font_data[original_y] >> original_x) & 0b1 == 0b1 {
                *font_3x_pixel = true;
            }
        }
    }

    for y in 0..24 {
        for x in 0..12 {
            // oo
            // oo -> black 0x00000
            //
            // 1o
            // oo -> dark grey
            //
            // 11
            // oo -> grey
            //
            // 11
            // 1o -> light grey
            //
            // 11
            // 11 -> white 0xffffff
            let mut true_count = 0;
            if font_3x[y * 2][x * 2] {
                true_count += 1;
            }
            if font_3x[y * 2 + 1][x * 2] {
                true_count += 1;
            }
            if font_3x[y * 2][x * 2 + 1] {
                true_count += 1;
            }
            if font_3x[y * 2 + 1][x * 2 + 1] {
                true_count += 1;
            }

            // y = ax;
            // 0 = a0;
            // 255 = a4;
            // 255 / 4 = 63.75
            // y = 63.75x;
            // y = 63.75 * true_count;
            let r = (color >> 16) & 0xff; // rgb 0xff / ff / ff
            let g = (color >> 8) & 0xff; // rgb 0xff / ff / ff
            let b = color & 0xff; // rgb 0xff / ff / ff
            draw_point(
                (((r * true_count) / 4) << 16)
                    | (((g * true_count) / 4) << 8)
                    | ((b * true_count) / 4),
                px + x as i64,
                py + y as i64,
            )?;
        }
    }
    Ok(())
}

// 1x
// ooo
// o1o
// ooo
//
// 3x
// ooooooooo
// ooooooooo
// ooooooooo
// ooo111ooo
// ooo111ooo
// ooo111ooo
// ooooooooo
// ooooooooo
// ooooooooo
pub fn draw_char_3x(color: u32, px: i64, py: i64, c: char) -> Result<()> {
    let font_data = BITMAP_FONT[c as usize];
    for y in 0..font_data.len() * 3 {
        for x in 0..24 {
            let original_x = x / 3;
            let original_y = y / 3;
            if (font_data[original_y] >> original_x) & 0b1 == 0b1 {
                draw_point(color, px + x, py + y as i64)?;
            }
        }
    }
    Ok(())
}

// 1x
// ooo
// o1o
// ooo
//
// 2x
// oooooo
// oooooo
// oo11oo
// oo11oo
// oooooo
// oooooo

// 1x
// o1 [1, 0]
// oo
//
// 2x
// oo11 [2,0] [3,0]
// oo11 [2,1] [3,1]
// oooo
// oooo
pub fn draw_char_2x(color: u32, px: i64, py: i64, c: char) -> Result<()> {
    let font_data = BITMAP_FONT[c as usize];
    for (y, row_bitmap) in font_data.iter().enumerate() {
        for x in 0..8 {
            if (row_bitmap >> x) & 0b1 == 0b1 {
                let y = y as i64;
                draw_point(color, px + x * 2, py + y * 2)?;
                draw_point(color, px + x * 2 + 1, py + y * 2)?;
                draw_point(color, px + x * 2, py + y * 2 + 1)?;
                draw_point(color, px + x * 2 + 1, py + y * 2 + 1)?;
            }
        }
    }
    Ok(())
}

// 1x:
// o1 [1, 0]
// oo
//
// 1.5x:
// o11
// o11
// ooo
//
// oo1
// oo1
// ooo
//
// 1x:
// oooo1
// ooooo
// ooooo
// ooooo
// ooooo
//
// 1.2x: (=> 12x => 縮小)
// ooooo1
// oooooo
// oooooo
// oooooo
// oooooo
// oooooo

/// Draws a character to the position of `x` and `y`. Upper case characters, lower case characters
/// and symbols are supported.
pub fn draw_char(color: u32, px: i64, py: i64, c: char) -> Result<()> {
    let font_data = BITMAP_FONT[c as usize];
    for (y, row_bitmap) in font_data.iter().enumerate() {
        for x in 0..8 {
            if (row_bitmap >> x) & 0b1 == 0b1 {
                draw_point(color, px + x, py + y as i64)?;
            }
        }
    }
    Ok(())
}

pub fn fill_circle(color: u32, center_x: i64, center_y: i64, radius: i64) -> Result<()> {
    for i in 0..radius * 2 + 1 {
        for j in 0..radius * 2 + 1 {
            let x = i - radius;
            let y = j - radius;

            if x * x + y * y <= radius * radius + 1 {
                draw_point(color, i + center_x, j + center_y)?;
            }
        }
    }
    Ok(())
}

pub fn fill_rect(color: u32, px: i64, py: i64, width: i64, height: i64) -> Result<()> {
    for dx in 0..width {
        for dy in 0..height {
            draw_point(color, px + dx, py + dy)?;
        }
    }
    Ok(())
}

pub fn draw_rect(color: u32, x: i64, y: i64, width: i64, height: i64) -> Result<()> {
    draw_line(color, x, y, x + width, y)?;
    draw_line(color, x, y, x, y + height)?;
    draw_line(color, x + width, y, x + width, y + height)?;
    draw_line(color, x, y + height, x + width, y + height)?;
    Ok(())
}

pub fn draw_line(color: u32, x0: i64, y0: i64, x1: i64, y1: i64) -> Result<()> {
    if x1 < x0 {
        return draw_line(color, x1, y1, x0, y0);
    }
    if x1 == x0 {
        if y0 <= y1 {
            for i in y0..=y1 {
                draw_point(color, x0, i)?;
            }
        } else {
            for i in y1..=y0 {
                draw_point(color, x0, i)?;
            }
        }
        return Ok(());
    }
    assert!(x0 < x1);
    let lx = x1 - x0 + 1;
    const MULTIPLIER: i64 = 1024 * 1024;
    let a = (y1 - y0) * MULTIPLIER / lx;
    for i in 0..lx {
        draw_line(
            color,
            x0 + i,
            y0 + (a * i / MULTIPLIER),
            x0 + i,
            y0 + (a * (i + 1) / MULTIPLIER),
        )?;
    }
    draw_point(color, x0, y0)?;
    draw_point(color, x1, y1)?;
    Ok(())
}

pub fn draw_point(c: u32, x: i64, y: i64) -> Result<()> {
    let result = crate::sys::draw_point(x, y, c);
    if result == 0 {
        Ok(())
    } else {
        Err(Error::Failed("draw_point: syscall failed"))
    }
}
