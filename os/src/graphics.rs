pub trait BitmapImageBuffer {
    fn bytes_per_pixel(&self) -> i64;
    fn pixels_per_line(&self) -> i64;
    fn width(&self) -> i64;
    fn height(&self) -> i64;
    fn buf(&self) -> *mut u8;
    /// # Safety
    ///
    /// This function should not be called for out of range coordinates.
    unsafe fn pixel_at(&self, x: i64, y: i64) -> *mut u8;
    fn flush(&self);
    fn is_in_x_range(&self, py: i64) -> bool;
    fn is_in_y_range(&self, py: i64) -> bool;
}

#[derive(Debug)]
pub enum GraphicsError {
    OutOfRange,
}

pub type GraphicsResult = core::result::Result<(), GraphicsError>;

#[allow(clippy::many_single_char_names)]
pub fn draw_point<T: BitmapImageBuffer>(buf: &T, color: u32, x: i64, y: i64) -> GraphicsResult {
    if !buf.is_in_x_range(x) || !buf.is_in_x_range(x) {
        return Err(GraphicsError::OutOfRange);
    }
    let r: u8 = (color >> 16) as u8;
    let g: u8 = (color >> 8) as u8;
    let b: u8 = color as u8;
    unsafe {
        let p = buf.pixel_at(x, y);
        // ARGB
        // *p.add(3) = alpha;
        *p.add(2) = r;
        *p.add(1) = g;
        *p.add(0) = b;
    }
    Ok(())
}

pub fn draw_line<T: BitmapImageBuffer>(
    buf: &T,
    color: u32,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
) -> GraphicsResult {
    if !buf.is_in_x_range(x0)
        || !buf.is_in_x_range(x1)
        || !buf.is_in_y_range(y0)
        || !buf.is_in_y_range(y1)
    {
        return Err(GraphicsError::OutOfRange);
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

pub fn draw_rect<T: BitmapImageBuffer>(
    buf: &T,
    color: u32,
    px: i64,
    py: i64,
    w: i64,
    h: i64,
) -> GraphicsResult {
    if !buf.is_in_x_range(px)
        || !buf.is_in_y_range(py)
        || !buf.is_in_x_range(px + w - 1)
        || !buf.is_in_y_range(py + h - 1)
    {
        return Err(GraphicsError::OutOfRange);
    }

    for y in py..py + h {
        for x in px..px + w {
            draw_point(buf, color, x, y)?;
        }
    }

    Ok(())
}

include!("../../generated/font.rs");

pub fn draw_char<T: BitmapImageBuffer>(
    buf: &T,
    fg_color: u32,
    bg_color: u32,
    px: i64,
    py: i64,
    c: char,
) -> GraphicsResult {
    if !buf.is_in_x_range(px)
        || !buf.is_in_y_range(py)
        || !buf.is_in_x_range(px + 8 - 1)
        || !buf.is_in_y_range(py + 16 - 1)
    {
        return Err(GraphicsError::OutOfRange);
    }

    let idx = c as usize;
    for y in 0..16_i64 {
        for x in 0..8_i64 {
            let col = if idx >= 256 || ((BITMAP_FONT[idx][y as usize] >> x) & 1) == 1 {
                fg_color
            } else {
                bg_color
            };
            draw_point(buf, col, px + x, py + y)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test_case]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
