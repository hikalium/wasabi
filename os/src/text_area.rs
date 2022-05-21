use crate::graphics::*;
use core::fmt;

pub struct TextArea<T: BitmapImageBuffer> {
    buf: T,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    cx: i64,
    cy: i64,
}

impl<T: BitmapImageBuffer> TextArea<T> {
    pub fn new(buf: T, x: i64, y: i64, w: i64, h: i64) -> TextArea<T> {
        let mut text_area = TextArea {
            buf,
            x,
            y,
            w,
            h,
            cx: 0,
            cy: 0,
        };
        text_area.clear_screen().unwrap();
        text_area
    }
    fn clear_screen(&mut self) -> GraphicsResult {
        draw_rect(&mut self.buf, 0x000000, self.x, self.y, self.w, self.h)
    }
    fn new_line(&mut self) -> GraphicsResult {
        self.cx = 0;
        self.cy += 1;
        if (self.cy + 1) * 16 <= self.h {
            return Ok(());
        }
        // Scroll!
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
        draw_rect(
            &mut self.buf,
            0x000000,
            self.x,
            self.y + self.cy * 16,
            self.w,
            16,
        )?;
        Ok(())
    }
    fn advance_cursor(&mut self) -> GraphicsResult {
        self.cx += 1;
        if self.cx * 8 + 8 <= self.w {
            return Ok(());
        }
        self.new_line()
    }
    pub fn print_char_with_color(&mut self, c: char, fg: u32, bg: u32) -> GraphicsResult {
        match c {
            '\n' => self.new_line(),
            _ => draw_char(
                &mut self.buf,
                fg,
                bg,
                self.x + self.cx * 8,
                self.y + self.cy * 16,
                c,
            )
            .and_then(|_| -> GraphicsResult { self.advance_cursor() }),
        }
    }
    pub fn print_char(&mut self, c: char) -> GraphicsResult {
        self.print_char_with_color(c, 0xFFFFFF, 0x000000)
    }
    pub fn print_string_with_color(&mut self, s: &str, fg: u32, bg: u32) -> GraphicsResult {
        for c in s.chars() {
            self.print_char_with_color(c, fg, bg)?;
        }
        Ok(())
    }
    pub fn print_string(&mut self, s: &str) -> GraphicsResult {
        self.print_string_with_color(s, 0xFFFFFF, 0x000000)
    }
}

impl<T: BitmapImageBuffer> fmt::Write for TextArea<T> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.print_string(s).or(Err(fmt::Error))?;
        Ok(())
    }
}
