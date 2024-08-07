use crate::bitmap::bitmap_draw_char;
use crate::bitmap::bitmap_draw_line;
use crate::bitmap::bitmap_draw_rect;
use crate::bitmap::transfer_rect;
use crate::bitmap::Bitmap;
use crate::error::Result;
use core::cmp::max;
use core::fmt;

pub enum TextAreaMode {
    Scroll,
    Ring,
}

pub struct TextArea<T: Bitmap> {
    buf: T,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    cx: i64,
    cy: i64,
    mode: TextAreaMode,
    ring_count: usize,
}

impl<T: Bitmap> TextArea<T> {
    pub fn new(buf: T, x: i64, y: i64, w: i64, h: i64) -> TextArea<T> {
        let mut text_area = TextArea {
            buf,
            x,
            y,
            w,
            h,
            cx: 0,
            cy: 0,
            mode: TextAreaMode::Scroll,
            ring_count: 0,
        };
        text_area.clear_screen().unwrap();
        text_area
    }
    fn clear_screen(&mut self) -> Result<()> {
        bitmap_draw_rect(&mut self.buf, 0x000000, self.x, self.y, self.w, self.h)
    }
    pub fn set_mode(&mut self, mode: TextAreaMode) {
        self.mode = mode;
    }
    fn new_line(&mut self) -> Result<()> {
        self.cx = 0;
        match self.mode {
            TextAreaMode::Scroll => {
                self.cy += 1;
                if (self.cy + 1) * 16 <= self.h {
                    return Ok(());
                }
                self.cy -= 1;
                transfer_rect(
                    &mut self.buf,
                    self.x,
                    self.y,
                    self.x,
                    self.y + 16,
                    self.w,
                    self.cy * 16,
                )?;
                bitmap_draw_rect(
                    &mut self.buf,
                    0x000000,
                    self.x,
                    self.y + self.cy * 16,
                    self.w,
                    16,
                )?;
            }
            TextAreaMode::Ring => {
                bitmap_draw_line(
                    &mut self.buf,
                    0xff << ((self.ring_count % 3) * 8),
                    self.x,
                    self.y + self.cy * 16,
                    self.x,
                    self.y + (self.cy + 1) * 16 - 1,
                )?;
                self.cy += 1;
                if (self.cy + 1) * 16 > self.h {
                    self.cy = 0;
                    self.ring_count += 1;
                }
                bitmap_draw_rect(
                    &mut self.buf,
                    0x000000,
                    self.x,
                    self.y + self.cy * 16,
                    self.w,
                    16,
                )?;
            }
        }
        Ok(())
    }
    fn move_cursor_next(&mut self) -> Result<()> {
        self.cx += 1;
        if self.cx * 8 + 8 <= self.w {
            return Ok(());
        }
        self.new_line()
    }
    fn move_cursor_prev(&mut self) -> Result<()> {
        self.cx = max(0, self.cx - 1);
        Ok(())
    }
    pub fn print_char_with_color(&mut self, c: char, fg: u32, bg: u32) -> Result<()> {
        match c {
            '\n' => self.new_line(),
            '\x08' | '\x7f' => self.move_cursor_prev(),
            _ => bitmap_draw_char(
                &mut self.buf,
                fg,
                Some(bg),
                self.x + self.cx * 8,
                self.y + self.cy * 16,
                c,
            )
            .and_then(|_| -> Result<()> { self.move_cursor_next() }),
        }
    }
    pub fn print_char(&mut self, c: char) -> Result<()> {
        self.print_char_with_color(c, 0xFFFFFF, 0x000000)
    }
    pub fn print_string_with_color(&mut self, s: &str, fg: u32, bg: u32) -> Result<()> {
        for c in s.chars() {
            self.print_char_with_color(c, fg, bg)?;
        }
        Ok(())
    }
    pub fn print_string(&mut self, s: &str) -> Result<()> {
        self.print_string_with_color(s, 0xFFFFFF, 0x000000)
    }
}

impl<T: Bitmap> fmt::Write for TextArea<T> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.print_string(s).or(Err(fmt::Error))?;
        Ok(())
    }
}
