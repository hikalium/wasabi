extern crate alloc;

use crate::graphics::Bitmap;
use alloc::vec::Vec;

#[derive(PartialEq, Eq, Debug)]
pub struct BitmapBuffer {
    buf: Vec<u8>,
    width: i64,
    height: i64,
    pixels_per_line: i64,
}
impl BitmapBuffer {
    pub fn new(width: i64, height: i64, pixels_per_line: i64) -> Self {
        assert!(width >= 0);
        assert!(height >= 0);
        assert!(pixels_per_line >= 0);
        assert!(pixels_per_line >= width);
        let mut buf = Self {
            buf: Vec::new(),
            width,
            height,
            pixels_per_line,
        };
        buf.buf.resize((pixels_per_line * height * 4) as usize, 0);
        buf
    }
}
impl Bitmap for BitmapBuffer {
    fn bytes_per_pixel(&self) -> i64 {
        4
    }
    fn pixels_per_line(&self) -> i64 {
        self.pixels_per_line
    }
    fn width(&self) -> i64 {
        self.width
    }
    fn height(&self) -> i64 {
        self.height
    }
    fn buf_mut(&mut self) -> *mut u8 {
        self.buf.as_mut_ptr()
    }
}
