use crate::result::Result;
use core::cmp::min;

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
        self.buf_mut()
            .add(((y * self.pixels_per_line() + x) * self.bytes_per_pixel()) as usize)
            as *mut u32
    }
    fn pixel_at_mut(&mut self, x: i64, y: i64) -> Option<&mut u32> {
        if self.is_in_x_range(x) && self.is_in_y_range(y) {
            // # Safety
            // (x, y) is always validated by the checks above.
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

unsafe fn unchecked_draw_point<T: Bitmap>(buf: &mut T, color: u32, x: i64, y: i64) -> Result<()> {
    // Assumes the buffer uses ARGB format
    // which means that the components will be stored in [A, R, G, B] order.
    // This is true for little-endian machine but not on big endian.
    *buf.unchecked_pixel_at_mut(x, y) = color;

    Ok(())
}
fn draw_point<T: Bitmap>(buf: &mut T, color: u32, x: i64, y: i64) -> Result<()> {
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
                unchecked_draw_point(buf, color, x, y)?;
            }
        }
    }

    Ok(())
}

fn draw_line<T: Bitmap>(buf: &mut T, color: u32, x0: i64, y0: i64, x1: i64, y1: i64) -> Result<()> {
    if !buf.is_in_x_range(x0)
        || !buf.is_in_x_range(x1)
        || !buf.is_in_y_range(y0)
        || !buf.is_in_y_range(y1)
    {
        return Err("Out of Range");
    }

    if x1 < x0 {
        return draw_line(buf, color, x1, y1, x0, y0);
    }
    if x1 == x0 {
        if y0 <= y1 {
            for i in y0..=y1 {
                draw_point(buf, color, x0, i)?;
            }
        } else {
            for i in y1..=y0 {
                draw_point(buf, color, x0, i)?;
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
            buf,
            color,
            x0 + i,
            y0 + (a * i / MULTIPLIER),
            x0 + i,
            y0 + (a * (i + 1) / MULTIPLIER),
        )?;
    }
    draw_point(buf, color, x0, y0)?;
    draw_point(buf, color, x1, y1)?;
    Ok(())
}

fn lookup_font(c: char) -> Option<[[char; 8]; 16]> {
    const FONT_SOURCE: &str = include_str!("./font.txt");
    if let Ok(c) = u8::try_from(c) {
        let mut fi = FONT_SOURCE.split("\n");
        while let Some(line) = fi.next() {
            if let Some(line) = line.strip_prefix("0x") {
                if let Ok(idx) = u8::from_str_radix(line, 16) {
                    if idx != c {
                        continue;
                    }
                    let mut font = [['*'; 8]; 16];
                    for (y, line) in fi.clone().take(16).enumerate() {
                        for (x, c) in line.chars().enumerate() {
                            if let Some(e) = font[y].get_mut(x) {
                                *e = c;
                            }
                        }
                    }
                    return Some(font);
                }
            }
        }
    }
    None
}

pub fn draw_font_fg<T: Bitmap>(buf: &mut T, x: i64, y: i64, color: u32, c: char) {
    if let Some(font) = lookup_font(c) {
        for (dy, row) in font.iter().enumerate() {
            for (dx, pixel) in row.iter().enumerate() {
                let color = match pixel {
                    '*' => color,
                    _ => continue,
                };
                let _ = draw_point(buf, color, x + dx as i64, y + dy as i64);
            }
        }
    }
}

fn draw_str_fg<T: Bitmap>(buf: &mut T, x: i64, y: i64, color: u32, s: &str) {
    for (i, c) in s.chars().enumerate() {
        draw_font_fg(buf, x + i as i64 * 8, y, color, c)
    }
}

pub fn draw_test_pattern<T: Bitmap>(buf: &mut T, px: i64, py: i64) -> Result<()> {
    // 128x128
    fill_rect(buf, 0x0000ff, px, py, 128, 128)?;
    fill_rect(buf, 0x00ff00, px, py, 64, 64)?;
    fill_rect(buf, 0xff0000, px, py, 32, 32)?;
    for i in 0..128 {
        let _ = draw_point(buf, 0x020200 * i as u32, px + i, py + i);
    }
    let px = px + 64;
    let py = py + 64;
    for i in (0..=63).step_by(9) {
        let _ = draw_line(buf, 0x00ffff, px, py + 63, px + 63, py + i);
        let _ = draw_line(buf, 0x00ffff, px + 63, py, px + i, py + 63);
    }
    let px = px - 24;
    draw_str_fg(buf, px, py, 0xffffff, "ABCDEF");
    Ok(())
}
