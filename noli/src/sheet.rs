extern crate alloc;

use crate::bitmap::Bitmap;
use crate::bitmap::BitmapBuffer;
use crate::error::Result;
use crate::graphics::draw_line;
use crate::graphics::draw_point;
use crate::rect::Rect;

#[derive(PartialEq, Eq, Debug)]
pub struct Sheet {
    x: i64,
    y: i64,
    bitmap: BitmapBuffer,
}

impl Sheet {
    pub fn new(rect: Rect) -> Self {
        Self {
            x: rect.x(),
            y: rect.y(),
            bitmap: BitmapBuffer::new(rect.w(), rect.h(), rect.w()),
        }
    }

    pub fn set_position(&mut self, x: i64, y: i64) {
        self.x = x;
        self.y = y;
    }

    pub fn x(&self) -> i64 {
        self.x
    }

    pub fn y(&self) -> i64 {
        self.y
    }

    pub fn width(&self) -> i64 {
        self.bitmap.width()
    }

    pub fn height(&self) -> i64 {
        self.bitmap.height()
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.bitmap.width(), self.bitmap.height()).unwrap()
    }

    pub fn bitmap(&mut self) -> &mut BitmapBuffer {
        &mut self.bitmap
    }

    // `rect` is absolute position.
    pub fn flush_area(&self, global_rect: Rect) {
        let global_intersection_rect = match self.rect().intersection(&global_rect) {
            Some(r) => r,
            None => return,
        };
        let intersection_rect = Rect::new(
            global_intersection_rect.x() - self.x,
            global_intersection_rect.y() - self.y,
            global_intersection_rect.w(),
            global_intersection_rect.h(),
        )
        .unwrap();
        let (x_range, y_range) = intersection_rect.frame_ranges();
        for y in y_range.range {
            for x in x_range.range.clone() {
                let p = self.bitmap.pixel_at(x, y).cloned().unwrap_or_default();
                let x = x + self.x;
                let y = y + self.y;
                let _ = draw_point(p, x, y);
            }
        }
    }

    pub fn flush(&self) {
        for y in 0..self.bitmap.height() {
            for x in 0..self.bitmap.width() {
                let p = self.bitmap.pixel_at(x, y).cloned().unwrap_or_default();
                let x = x + self.x;
                let y = y + self.y;
                let _ = draw_point(p, x, y);
            }
        }
    }

    pub fn draw_border(&mut self, color: u32) -> Result<()> {
        draw_line(color, self.x, self.y, self.x + self.bitmap.width(), self.y)?;
        draw_line(color, self.x, self.y, self.x, self.y + self.bitmap.height())?;
        draw_line(
            color,
            self.x + self.bitmap.width(),
            self.y,
            self.x + self.bitmap.width(),
            self.y + self.bitmap.height(),
        )?;
        draw_line(
            color,
            self.x,
            self.y + self.bitmap.height(),
            self.x + self.bitmap.width(),
            self.y + self.bitmap.height(),
        )?;
        Ok(())
    }
}
