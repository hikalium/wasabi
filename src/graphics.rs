use crate::mutex::Mutex;
use crate::result::Result;
use core::cmp::max;
use core::cmp::min;
use core::fmt;
use core::ops::Range;

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

fn lookup_font(c: char) -> Option<[[char; 8]; 16]> {
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

pub fn draw_font_fg<T: Bitmap>(
    buf: &mut T,
    x: i64,
    y: i64,
    color: u32,
    c: char,
) {
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

pub fn draw_str_fg<T: Bitmap>(
    buf: &mut T,
    x: i64,
    y: i64,
    color: u32,
    s: &str,
) {
    for (i, c) in s.chars().enumerate() {
        draw_font_fg(buf, x + i as i64 * 8, y, color, c)
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

pub fn draw_button<T: Bitmap>(
    buf: &mut T,
    left: i64,
    top: i64,
    w: i64,
    h: i64,
    fgc: u32,
    is_pressed: bool,
) -> Result<()> {
    fill_rect(buf, fgc, left, top, w, h)?;
    let right = left + w - 1;
    let bottom = top + h - 1;
    draw_line(buf, 0xffffff * !is_pressed as u32, left, top, right, top)?;
    draw_line(buf, 0xffffff * !is_pressed as u32, left, top, left, bottom)?;
    draw_line(buf, 0xffffff * is_pressed as u32, right, bottom, right, top)?;
    draw_line(
        buf,
        0xffffff * is_pressed as u32,
        right,
        bottom,
        left,
        bottom,
    )?;
    Ok(())
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
    fn adjust_cursor_pos(&mut self) -> bool {
        let mut adjusted = false;
        let (w, h) = {
            let bmp = self.buf.lock();
            (bmp.width(), bmp.height())
        };
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
            }
            draw_font_fg(
                &mut *self.buf.lock(),
                self.cursor_x,
                self.cursor_y,
                0xffffff,
                c,
            );
            self.cursor_x += 8;
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

/// A non-empty i64 scalar range, for graphics operations
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct ScalarRange {
    pub range: Range<i64>,
}
impl ScalarRange {
    pub fn new(start: i64, end: i64) -> Option<Self> {
        let range = start..end;
        if range.is_empty() {
            None
        } else {
            Some(Self { range })
        }
    }
    pub fn intersection(&self, another: &Self) -> Option<Self> {
        let range = max(self.range.start, another.range.start)
            ..min(self.range.end, another.range.end);
        Self::new(range.start, range.end)
    }
    pub fn start(&self) -> i64 {
        self.range.start
    }
    pub fn end(&self) -> i64 {
        self.range.end
    }
}
#[cfg(test)]
mod scalar_range_tests {
    use super::ScalarRange;
    #[test_case]
    fn creates_range() {
        let r = ScalarRange::new(0, 0);
        assert!(r.is_none());
        let r = ScalarRange::new(1, 0);
        assert!(r.is_none());
        let r = ScalarRange::new(0, 1).unwrap();
        assert_eq!(r.start(), 0);
        assert_eq!(r.end(), 1);
    }
    #[test_case]
    fn intersections() {
        let a = ScalarRange::new(2, 3).unwrap();
        let b = ScalarRange::new(1, 2).unwrap();
        let c = ScalarRange::new(3, 4).unwrap();
        let d = ScalarRange::new(1, 4).unwrap();
        let e = ScalarRange::new(0, 2).unwrap();
        let f = ScalarRange::new(3, 5).unwrap();

        assert!(a.intersection(&a).unwrap() == a);
        assert!(a.intersection(&b).is_none());
        assert!(a.intersection(&c).is_none());
        assert!(a.intersection(&d).unwrap() == a);
        assert!(a.intersection(&e).is_none());
        assert!(a.intersection(&f).is_none());
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct Rect {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}
impl Rect {
    pub fn new(x: i64, y: i64, w: i64, h: i64) -> Option<Rect> {
        if w < 0 || h < 0 {
            None
        } else {
            Some(Self { x, y, w, h })
        }
    }
    pub fn x(&self) -> i64 {
        self.x
    }
    pub fn y(&self) -> i64 {
        self.y
    }
    pub fn w(&self) -> i64 {
        self.w
    }
    pub fn h(&self) -> i64 {
        self.h
    }
    pub fn frame_ranges(&self) -> (ScalarRange, ScalarRange) {
        (
            ScalarRange::new(self.x, self.x + self.w).unwrap(),
            ScalarRange::new(self.y, self.y + self.h).unwrap(),
        )
    }
    pub fn intersection(&self, another: &Self) -> Option<Rect> {
        let (rx0, ry0) = self.frame_ranges();
        let (rx1, ry1) = another.frame_ranges();
        let rx = rx0.intersection(&rx1)?;
        let ry = ry0.intersection(&ry1)?;
        let x = rx.start();
        let w = rx.end() - rx.start();
        let y = ry.start();
        let h = ry.end() - ry.start();
        Some(Self { x, y, w, h })
    }
    pub fn contains_point(&self, x: i64, y: i64) -> bool {
        let (rx, ry) = self.frame_ranges();
        rx.range.contains(&x) && ry.range.contains(&y)
    }
}

#[cfg(test)]
mod rect_tests {
    use super::Rect;

    #[test_case]
    fn creates_rect() {
        let r = Rect::new(0, 0, 0, 0).unwrap();
        assert_eq!(r.x(), 0);
        assert_eq!(r.y(), 0);
        assert_eq!(r.w(), 0);
        assert_eq!(r.h(), 0);

        let r = Rect::new(1, 2, 3, 4).unwrap();
        assert_eq!(r.x(), 1);
        assert_eq!(r.y(), 2);
        assert_eq!(r.w(), 3);
        assert_eq!(r.h(), 4);

        let r = Rect::new(-1, -2, 3, 4).unwrap();
        assert_eq!(r.x(), -1);
        assert_eq!(r.y(), -2);
        assert_eq!(r.w(), 3);
        assert_eq!(r.h(), 4);
    }
    #[test_case]
    fn fails_to_create_negative_sized_rect() {
        assert!(Rect::new(0, 0, -1, 0).is_none());
        assert!(Rect::new(0, 0, 0, -1).is_none());
        assert!(Rect::new(0, 0, -1, -1).is_none());
    }
    #[test_case]
    fn calc_intersection() {
        let r1 = Rect::new(0, 0, 1, 1).unwrap();
        let self_intersect = r1.intersection(&r1).unwrap();
        assert_eq!(self_intersect, r1);
    }
}
