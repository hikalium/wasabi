use crate::font::get_glyph_width;
use crate::font::lookup_font_16x16;
use crate::font::lookup_font_8x16;
use crate::result::Result;
use core::cmp::min;
use core::fmt;

pub trait Bitmap {
    fn bytes_per_pixel(&self) -> i64;
    fn pixels_per_line(&self) -> i64;
    fn width(&self) -> i64;
    fn height(&self) -> i64;
    fn buf_mut(&mut self) -> *mut u8;
    /// # Safety
    ///
    /// Returned pointer is valid as long as the given coordinates are valid
    /// which means that passing is_in_*_range tests.
    unsafe fn unchecked_pixel_at_mut(&mut self, x: i64, y: i64) -> *mut u32 {
        self.buf_mut().add(
            ((y * self.pixels_per_line() + x) * self.bytes_per_pixel())
                as usize,
        ) as *mut u32
    }
    fn pixel_at_mut(&mut self, x: i64, y: i64) -> Option<&mut u32> {
        if self.is_in_x_range(x) && self.is_in_y_range(y) {
            // SAFETY: (x, y) is always validated by the checks above.
            unsafe { Some(&mut *(self.unchecked_pixel_at_mut(x, y))) }
        } else {
            None
        }
    }
    fn is_in_x_range(&self, px: i64) -> bool {
        0 <= px && px < min(self.width(), self.pixels_per_line())
    }
    fn is_in_y_range(&self, py: i64) -> bool {
        0 <= py && py < self.height()
    }
}

/// # Safety
///
/// (x, y) must be a valid point in the buf.
unsafe fn unchecked_draw_point<T: Bitmap>(
    buf: &mut T,
    color: u32,
    x: i64,
    y: i64,
) {
    *buf.unchecked_pixel_at_mut(x, y) = color;
}

fn draw_point<T: Bitmap>(
    buf: &mut T,
    color: u32,
    x: i64,
    y: i64,
) -> Result<()> {
    *(buf.pixel_at_mut(x, y).ok_or("Out of Range")?) = color;
    Ok(())
}

pub fn fill_rect<T: Bitmap>(
    buf: &mut T,
    color: u32,
    px: i64,
    py: i64,
    w: i64,
    h: i64,
) -> Result<()> {
    if !buf.is_in_x_range(px)
        || !buf.is_in_y_range(py)
        || !buf.is_in_x_range(px + w - 1)
        || !buf.is_in_y_range(py + h - 1)
    {
        return Err("Out of Range");
    }
    for y in py..py + h {
        for x in px..px + w {
            unsafe {
                unchecked_draw_point(buf, color, x, y);
            }
        }
    }
    Ok(())
}

fn calc_slope_point(da: i64, db: i64, ia: i64) -> Option<i64> {
    if da < db {
        None
    } else if da == 0 {
        Some(0)
    } else if (0..=da).contains(&ia) {
        Some((2 * db * ia + da) / da / 2)
    } else {
        None
    }
}

fn draw_line<T: Bitmap>(
    buf: &mut T,
    color: u32,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
) -> Result<()> {
    if !buf.is_in_x_range(x0)
        || !buf.is_in_x_range(x1)
        || !buf.is_in_y_range(y0)
        || !buf.is_in_y_range(y1)
    {
        return Err("Out of Range");
    }
    let dx = (x1 - x0).abs();
    let sx = (x1 - x0).signum();
    let dy = (y1 - y0).abs();
    let sy = (y1 - y0).signum();
    if dx >= dy {
        for (rx, ry) in (0..dx)
            .flat_map(|rx| calc_slope_point(dx, dy, rx).map(|ry| (rx, ry)))
        {
            draw_point(buf, color, x0 + rx * sx, y0 + ry * sy)?;
        }
    } else {
        for (rx, ry) in (0..dy)
            .flat_map(|ry| calc_slope_point(dy, dx, ry).map(|rx| (rx, ry)))
        {
            draw_point(buf, color, x0 + rx * sx, y0 + ry * sy)?;
        }
    }
    Ok(())
}

/// Returns the glyph width in pixels.
pub fn draw_font_fg<T: Bitmap>(
    buf: &mut T,
    x: i64,
    y: i64,
    color: u32,
    c: char,
) -> i64 {
    if let Some(font) = lookup_font_8x16(c) {
        for (dy, row) in font.iter().enumerate() {
            for (dx, pixel) in row.iter().enumerate() {
                let color = match pixel {
                    '*' => color,
                    _ => continue,
                };
                let _ = draw_point(buf, color, x + dx as i64, y + dy as i64);
            }
        }
        8
    } else if let Some(font) = lookup_font_16x16(c) {
        for (dy, row) in font.iter().enumerate() {
            for (dx, pixel) in row.iter().enumerate() {
                let color = match pixel {
                    '#' => color,
                    _ => continue,
                };
                let _ = draw_point(buf, color, x + dx as i64, y + dy as i64);
            }
        }
        16
    } else {
        0
    }
}

pub fn draw_str_fg<T: Bitmap>(
    buf: &mut T,
    mut x: i64,
    y: i64,
    color: u32,
    s: &str,
) {
    for c in s.chars() {
        let dx = draw_font_fg(buf, x, y, color, c);
        x += dx;
    }
}

pub fn draw_test_pattern<T: Bitmap>(buf: &mut T) {
    let w = 128;
    let left = buf.width() - w - 1;
    let colors = [0x000000, 0xff0000, 0x00ff00, 0x0000ff];
    let h = 64;
    for (i, c) in colors.iter().enumerate() {
        let y = i as i64 * h;
        fill_rect(buf, *c, left, y, h, h).expect("fill_rect failed");
        fill_rect(buf, !*c, left + h, y, h, h).expect("fill_rect failed");
    }
    let points = [(0, 0), (0, w), (w, 0), (w, w)];
    for (x0, y0) in points.iter() {
        for (x1, y1) in points.iter() {
            let _ = draw_line(buf, 0xffffff, left + *x0, *y0, left + *x1, *y1);
        }
    }
    draw_str_fg(buf, left, h * colors.len() as i64, 0x00ff00, "0123456789");
    draw_str_fg(buf, left, h * colors.len() as i64 + 16, 0x00ff00, "ABCDEF");
}

pub struct BitmapTextWriter<T> {
    buf: T,
    cursor_x: i64,
    cursor_y: i64,
}
impl<T: Bitmap> BitmapTextWriter<T> {
    pub fn new(buf: T) -> Self {
        Self {
            buf,
            cursor_x: 0,
            cursor_y: 0,
        }
    }
    pub fn buf(&self) -> &T {
        &self.buf
    }
    fn adjust_cursor_pos_pre(&mut self, next_glyph_width: i64) -> bool {
        let mut adjusted = false;
        let (w, h) = (self.buf.width(), self.buf.height());
        if self.cursor_x + next_glyph_width > w {
            self.cursor_x = 0;
            self.cursor_y += 16;
            adjusted = true;
        }
        if self.cursor_y >= h {
            self.cursor_y = 0;
            adjusted = true;
        }
        adjusted
    }
    fn adjust_cursor_pos(&mut self) -> bool {
        let mut adjusted = false;
        if self.cursor_x < 0 {
            self.cursor_x = 0;
            adjusted = true;
        }
        if self.cursor_x >= self.buf.width() {
            self.cursor_x = 0;
            self.cursor_y += 16;
            adjusted = true;
        }
        if self.cursor_y >= self.buf.height() {
            self.cursor_y = 0;
            adjusted = true;
        }
        adjusted
    }
}
impl<T: Bitmap> fmt::Write for BitmapTextWriter<T> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let w = self.buf.width();
        for c in s.chars() {
            if c == '\n' {
                self.cursor_y += 16;
                self.cursor_x = 0;
                self.adjust_cursor_pos();
                fill_rect(&mut self.buf, 0x000000, 0, self.cursor_y, w, 16)
                    .or(Err(fmt::Error))?;
                continue;
            } else if c == '\x08' {
                self.cursor_x -= 8;
                self.adjust_cursor_pos();
                fill_rect(
                    &mut self.buf,
                    0x000000,
                    self.cursor_x,
                    self.cursor_y,
                    8,
                    16,
                )
                .or(Err(fmt::Error))?;
                continue;
            }
            let gw = get_glyph_width(c);
            if self.adjust_cursor_pos_pre(gw) {
                fill_rect(&mut self.buf, 0x000000, 0, self.cursor_y, w, 16)
                    .or(Err(fmt::Error))?;
            }

            let dx = draw_font_fg(
                &mut self.buf,
                self.cursor_x,
                self.cursor_y,
                0xffffff,
                c,
            );
            self.cursor_x += dx;
            if self.adjust_cursor_pos() {
                fill_rect(&mut self.buf, 0x000000, 0, self.cursor_y, w, 16)
                    .or(Err(fmt::Error))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod font_drawing_tests {
    use super::*;
    use crate::bitmap::BitmapBuffer;

    const FG: u32 = 0xffffff;

    fn new_bitmap(width: i64) -> BitmapBuffer {
        BitmapBuffer::new(width, 16, width)
    }

    fn count_fg_pixels(buf: &mut BitmapBuffer, x0: i64, x1: i64) -> usize {
        let mut n = 0;
        for y in 0..buf.height() {
            for x in x0..x1 {
                if matches!(buf.pixel_at_mut(x, y), Some(&mut c) if c == FG) {
                    n += 1;
                }
            }
        }
        n
    }

    #[test_case]
    fn draw_font_fg_returns_8_for_an_ascii_glyph() {
        let mut buf = new_bitmap(32);
        assert_eq!(draw_font_fg(&mut buf, 0, 0, FG, 'A'), 8);
        assert!(count_fg_pixels(&mut buf, 0, 8) > 0);
        assert_eq!(count_fg_pixels(&mut buf, 8, 32), 0);
    }

    #[test_case]
    fn draw_font_fg_returns_16_for_a_wide_glyph() {
        let mut buf = new_bitmap(32);
        assert_eq!(draw_font_fg(&mut buf, 0, 0, FG, '日'), 16);
        assert!(count_fg_pixels(&mut buf, 8, 16) > 0);
        assert_eq!(count_fg_pixels(&mut buf, 16, 32), 0);
    }

    #[test_case]
    fn draw_font_fg_returns_0_for_a_glyph_not_in_the_font() {
        // U+FFFF is a noncharacter: it is permanently unassigned, so no
        // font will ever have a glyph for it.
        const NO_GLYPH: char = '\u{FFFF}';
        let mut buf = new_bitmap(32);
        assert_eq!(draw_font_fg(&mut buf, 0, 0, FG, NO_GLYPH), 0);
        assert_eq!(count_fg_pixels(&mut buf, 0, 32), 0);
    }

    // Asserts that draw_str_fg puts each glyph of s exactly where drawing
    // it alone at the given x would: same pixels, nothing else.
    fn assert_glyph_layout(s: &str, offsets: &[i64]) {
        let mut drawn = new_bitmap(64);
        draw_str_fg(&mut drawn, 0, 0, FG, s);
        let mut expected = new_bitmap(64);
        for (c, &x) in s.chars().zip(offsets) {
            draw_font_fg(&mut expected, x, 0, FG, c);
        }
        assert_eq!(drawn, expected);
    }

    #[test_case]
    fn draw_str_fg_lays_out_glyphs_left_to_right() {
        assert_glyph_layout("", &[]);
        assert_glyph_layout("A", &[0]);
        assert_glyph_layout("日", &[0]);
        assert_glyph_layout("AA", &[0, 8]);
        assert_glyph_layout("A日", &[0, 8]);
        assert_glyph_layout("日A", &[0, 16]);
        assert_glyph_layout("日日", &[0, 16]);
        assert_glyph_layout("AAA", &[0, 8, 16]);
        assert_glyph_layout("AA日", &[0, 8, 16]);
        assert_glyph_layout("A日A", &[0, 8, 24]);
        assert_glyph_layout("A日日", &[0, 8, 24]);
        assert_glyph_layout("日AA", &[0, 16, 24]);
        assert_glyph_layout("日A日", &[0, 16, 24]);
        assert_glyph_layout("日日A", &[0, 16, 32]);
        assert_glyph_layout("日日日", &[0, 16, 32]);
    }

    #[test_case]
    fn draw_str_fg_clips_glyphs_at_the_right_edge() {
        // A glyph that starts past the right edge draws nothing at all.
        let mut two = new_bitmap(16);
        draw_str_fg(&mut two, 0, 0, FG, "日日");
        let mut one = new_bitmap(16);
        draw_str_fg(&mut one, 0, 0, FG, "日");
        assert_eq!(two, one);

        // A glyph that straddles the edge draws the part that fits.
        let mut half = new_bitmap(24);
        draw_str_fg(&mut half, 0, 0, FG, "日日");
        assert!(count_fg_pixels(&mut half, 16, 24) > 0);
    }
}

#[cfg(test)]
mod bmp_text_writer_tests {
    use super::*;
    use crate::bitmap::BitmapBuffer;
    use core::fmt::Write;

    #[test_case]
    fn create_writer() {
        let bmp = BitmapBuffer::new(24, 32, 24);
        let writer = BitmapTextWriter::new(bmp);
        assert_eq!(writer.cursor_x, 0);
        assert_eq!(writer.cursor_y, 0);
    }

    #[test_case]
    fn write_char_w8() {
        let bmp = BitmapBuffer::new(24, 32, 24);
        let mut writer = BitmapTextWriter::new(bmp);
        write!(writer, "A").unwrap();
        assert_eq!(writer.cursor_x, 8);
        assert_eq!(writer.cursor_y, 0);
    }
    #[test_case]
    fn write_char_w16() {
        let bmp = BitmapBuffer::new(24, 32, 24);
        let mut writer = BitmapTextWriter::new(bmp);
        write!(writer, "あ").unwrap();
        assert_eq!(writer.cursor_x, 16);
        assert_eq!(writer.cursor_y, 0);
    }
    #[test_case]
    fn write_char_w16x2() {
        let bmp = BitmapBuffer::new(24, 32, 24);
        let mut writer = BitmapTextWriter::new(bmp);
        write!(writer, "ああ").unwrap();
        assert_eq!(writer.cursor_x, 16);
        assert_eq!(writer.cursor_y, 16);
    }
    #[test_case]
    fn write_char_w8x3() {
        let bmp = BitmapBuffer::new(24, 32, 24);
        let mut writer = BitmapTextWriter::new(bmp);
        write!(writer, "AAA").unwrap();
        assert_eq!(writer.cursor_x, 0);
        assert_eq!(writer.cursor_y, 16);
    }
    #[test_case]
    fn write_char_w8x4() {
        let bmp = BitmapBuffer::new(24, 32, 24);
        let mut writer = BitmapTextWriter::new(bmp);
        write!(writer, "AAAA").unwrap();
        assert_eq!(writer.cursor_x, 8);
        assert_eq!(writer.cursor_y, 16);
    }
    #[test_case]
    fn write_char_w8_w16() {
        let bmp = BitmapBuffer::new(24, 32, 24);
        let mut writer = BitmapTextWriter::new(bmp);
        write!(writer, "Aあ").unwrap();
        assert_eq!(writer.cursor_x, 0);
        assert_eq!(writer.cursor_y, 16);
    }
    #[test_case]
    fn write_char_w16_w8() {
        let bmp = BitmapBuffer::new(24, 32, 24);
        let mut writer = BitmapTextWriter::new(bmp);
        write!(writer, "あA").unwrap();
        assert_eq!(writer.cursor_x, 0);
        assert_eq!(writer.cursor_y, 16);
    }
    #[test_case]
    fn write_char_w8x6() {
        let bmp = BitmapBuffer::new(24, 32, 24);
        let mut writer = BitmapTextWriter::new(bmp);
        write!(writer, "AAAAAA").unwrap();
        assert_eq!(writer.cursor_x, 0);
        assert_eq!(writer.cursor_y, 0);
    }
    #[test_case]
    fn write_char_w16x3() {
        let bmp = BitmapBuffer::new(24, 32, 24);
        let mut writer = BitmapTextWriter::new(bmp);
        write!(writer, "あああ").unwrap();
        assert_eq!(writer.cursor_x, 16);
        assert_eq!(writer.cursor_y, 0);
    }
}
