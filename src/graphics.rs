use crate::font::get_glyph_width;
use crate::font::lookup_font_16x16;
use crate::font::lookup_font_8x16;
use crate::mutex::Mutex;
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
pub fn draw_point<T: Bitmap>(
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

pub struct BitmapTextWriter<'a, T> {
    buf: &'a Mutex<T>,
    cursor_x: i64,
    cursor_y: i64,
}
impl<'a, T: Bitmap> BitmapTextWriter<'a, T> {
    pub const fn new(buf: &'a Mutex<T>) -> Self {
        Self {
            buf,
            cursor_x: 0,
            cursor_y: 0,
        }
    }
    fn adjust_cursor_pos_pre(&mut self, next_glyph_width: i64) -> bool {
        let mut adjusted = false;
        let (w, h) = {
            let bmp = self.buf.lock();
            (bmp.width(), bmp.height())
        };
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
        let (w, h) = {
            let bmp = self.buf.lock();
            (bmp.width(), bmp.height())
        };
        if self.cursor_x < 0 {
            self.cursor_x = 0;
            adjusted = true;
        }
        if self.cursor_x >= w {
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
}
impl<'a, T: Bitmap> fmt::Write for BitmapTextWriter<'a, T> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let w = self.buf.lock().width();
        for c in s.chars() {
            if c == '\n' {
                self.cursor_y += 16;
                self.cursor_x = 0;
                self.adjust_cursor_pos();
                fill_rect(
                    &mut *self.buf.lock(),
                    0x000000,
                    0,
                    self.cursor_y,
                    w,
                    16,
                )
                .or(Err(fmt::Error))?;
                continue;
            } else if c == '\x08' {
                self.cursor_x -= 8;
                self.adjust_cursor_pos();
                fill_rect(
                    &mut *self.buf.lock(),
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
                fill_rect(
                    &mut *self.buf.lock(),
                    0x000000,
                    0,
                    self.cursor_y,
                    w,
                    16,
                )
                .or(Err(fmt::Error))?;
            }

            let dx = draw_font_fg(
                &mut *self.buf.lock(),
                self.cursor_x,
                self.cursor_y,
                0xffffff,
                c,
            );
            self.cursor_x += dx;
            if self.adjust_cursor_pos() {
                fill_rect(
                    &mut *self.buf.lock(),
                    0x000000,
                    0,
                    self.cursor_y,
                    w,
                    16,
                )
                .or(Err(fmt::Error))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod bmp_text_writer_tests {
    use super::*;
    use crate::bitmap::BitmapBuffer;
    use core::fmt::Write;

    #[test_case]
    fn create_writer() {
        let bmp = Mutex::new(BitmapBuffer::new(24, 32, 24));
        let writer = BitmapTextWriter::new(&bmp);
        assert_eq!(writer.cursor_x, 0);
        assert_eq!(writer.cursor_y, 0);
    }

    #[test_case]
    fn write_advances_cursor() {
        // On a 24x32 bitmap: an ASCII glyph is 8px wide, a wide glyph
        // is 16px, so 3 ASCII chars (or 24px worth) wrap to the next
        // 16px-tall line, and 32px of height wraps back to the top.
        // (input, expected cursor_x, expected cursor_y)
        let cases: &[(&str, i64, i64)] = &[
            ("A", 8, 0),
            ("あ", 16, 0),
            ("ああ", 16, 16), // wide-wide: the second one wraps
            ("AAA", 0, 16),   // the third ASCII char wraps
            ("AAAA", 8, 16),
            ("Aあ", 0, 16), // 8 + 16 > 24: the wide glyph wraps
            ("あA", 0, 16),
            ("AAAAAA", 0, 0), // wraps twice: back to the top line
            ("あああ", 16, 0),
        ];
        for (s, x, y) in cases {
            let bmp = Mutex::new(BitmapBuffer::new(24, 32, 24));
            let mut writer = BitmapTextWriter::new(&bmp);
            write!(writer, "{s}").unwrap();
            assert_eq!(writer.cursor_x, *x, "input: {s:?}");
            assert_eq!(writer.cursor_y, *y, "input: {s:?}");
        }
    }
}

