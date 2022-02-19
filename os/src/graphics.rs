pub trait BitmapImageBuffer {
    type Pixel = u32;
    fn bytes_per_pixel(&self) -> i64;
    fn pixels_per_line(&self) -> i64;
    fn width(&self) -> i64;
    fn height(&self) -> i64;
    fn buf(&self) -> *const u8;
    fn buf_mut(&mut self) -> *mut u8;
    fn pixel_at(&mut self, x: i64, y: i64) -> Option<&mut Self::Pixel> {
        if self.is_in_x_range(x) && self.is_in_y_range(y) {
            // # Safety
            // (x, y) is always validated by the checks above.
            unsafe { Some(&mut *(self.unchecked_pixel_at(x, y) as *mut Self::Pixel)) }
        } else {
            None
        }
    }
    /// # Safety
    ///
    /// Returned pointer is valid as long as the given coordinates are valid
    /// which means that passing is_in_*_range tests.
    unsafe fn unchecked_pixel_at(&mut self, x: i64, y: i64) -> *mut u8 {
        self.buf_mut()
            .add(((y * self.pixels_per_line() + x) * self.bytes_per_pixel()) as usize)
    }
    fn flush(&self) {
        // Do nothing
    }
    fn is_in_x_range(&self, px: i64) -> bool {
        use core::cmp::min;
        0 <= px && px < min(self.width(), self.pixels_per_line()) as i64
    }
    fn is_in_y_range(&self, py: i64) -> bool {
        0 <= py && py < self.height() as i64
    }
}

#[derive(Debug)]
pub enum GraphicsError {
    OutOfRange,
}

pub type GraphicsResult = core::result::Result<(), GraphicsError>;

#[allow(clippy::many_single_char_names)]
pub fn draw_point<T: BitmapImageBuffer>(buf: &mut T, color: u32, x: i64, y: i64) -> GraphicsResult {
    if !buf.is_in_x_range(x) || !buf.is_in_x_range(x) {
        return Err(GraphicsError::OutOfRange);
    }
    let r: u8 = (color >> 16) as u8;
    let g: u8 = (color >> 8) as u8;
    let b: u8 = color as u8;
    unsafe {
        let p = buf.unchecked_pixel_at(x, y);
        // ARGB
        // *p.add(3) = alpha;
        *p.add(2) = r;
        *p.add(1) = g;
        *p.add(0) = b;
    }
    Ok(())
}

pub fn draw_line<T: BitmapImageBuffer>(
    buf: &mut T,
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
    buf: &mut T,
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
    buf: &mut T,
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
    extern crate alloc;

    use super::*;
    use alloc::vec::Vec;

    pub struct TestBuffer {
        buf: Vec<u8>,
        width: i64,
        height: i64,
        pixels_per_line: i64,
    }

    impl TestBuffer {
        fn new(width: i64, height: i64, pixels_per_line: i64) -> Self {
            assert!(width >= 0);
            assert!(height >= 0);
            assert!(pixels_per_line >= 0);
            assert!(pixels_per_line >= width);
            let mut buf = TestBuffer {
                buf: Vec::new(),
                width,
                height,
                pixels_per_line,
            };
            buf.buf.resize((pixels_per_line * height * 4) as usize, 0);
            buf
        }
    }

    impl BitmapImageBuffer for TestBuffer {
        fn bytes_per_pixel(&self) -> i64 {
            4
        }
        fn pixels_per_line(&self) -> i64 {
            self.pixels_per_line as i64
        }
        fn width(&self) -> i64 {
            self.width as i64
        }
        fn height(&self) -> i64 {
            self.height as i64
        }
        fn buf(&self) -> *const u8 {
            self.buf.as_ptr()
        }
        fn buf_mut(&mut self) -> *mut u8 {
            self.buf.as_mut_ptr()
        }
    }
    #[test_case]
    fn test_buf_default() {
        let h = 13_i64;
        let w = 17_i64;
        let pixels_per_line = 19_i64;
        let mut buf = TestBuffer::new(w, h, pixels_per_line);
        assert!(buf.pixel_at(-1, 0) == None);
        assert!(buf.pixel_at(0, -1) == None);
        assert!(buf.pixel_at(w - 1, h) == None);
        assert!(buf.pixel_at(w, h - 1) == None);
        for y in 0..h {
            for x in 0..w {
                assert!(buf.pixel_at(x, y) == Some(&mut 0))
            }
        }
    }
    #[test_case]
    fn draw_rect_default() {
        let h = 13_i64;
        let w = 17_i64;
        let pixels_per_line = 19_i64;
        let mut buf = TestBuffer::new(w, h, pixels_per_line);
        assert!(draw_rect(&mut buf, 0xff0000, 0, 0, w, h).is_ok());
        for y in 0..h {
            for x in 0..w {
                assert!(buf.pixel_at(x, y) == Some(&mut 0xff0000))
            }
        }
    }
}
