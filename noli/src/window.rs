extern crate alloc;

use crate::bitmap::bitmap_draw_line;
use crate::bitmap::bitmap_draw_point;
use crate::bitmap::bitmap_draw_rect;
use crate::bitmap::bitmap_draw_string_no_bg;
use crate::bitmap::bitmap_draw_string_no_bg_with_underline;
use crate::error::Error;
use crate::error::Result;
use crate::rect::Rect;
use crate::sheet::Sheet;
use alloc::string::String;
use core::cmp::max;
use core::cmp::min;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::OriginDimensions;
use embedded_graphics::geometry::Size;
use embedded_graphics::prelude::RgbColor;
use embedded_graphics::Pixel;

static WHITE: u32 = 0xffffff;
static DARKBLUE: u32 = 0x00008b;
static GREY: u32 = 0x808080;
static DARKGREY: u32 = 0x5a5a5a;
static BLACK: u32 = 0x000000;

static TITLE_BAR_HEIGHT: i64 = 24;
static BUTTON_SIZE: i64 = 16;

#[derive(Clone, Debug)]
pub enum StringSize {
    // used for draw_string
    Medium,
    // used for draw_string_2x
    Large,
    // used for draw_string_3x
    XLarge,
}

/// Represent a window for one application.
#[derive(Debug)]
pub struct Window {
    name: String,
    _background_color: u32,
    sheet: Sheet,
    _active: bool,
}

/// https://docs.rs/embedded-graphics/latest/embedded_graphics/geometry/trait.OriginDimensions.html
impl OriginDimensions for Window {
    fn size(&self) -> Size {
        Size::new(self.sheet.width() as u32, self.sheet.height() as u32)
    }
}

/// https://docs.rs/embedded-graphics/latest/embedded_graphics/draw_target/trait.DrawTarget.html
impl DrawTarget for Window {
    type Color = embedded_graphics::pixelcolor::Rgb888;
    type Error = Error;
    fn draw_iter<I>(&mut self, pixels: I) -> Result<()>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x >= self.sheet.width() as i32
                || point.y + TITLE_BAR_HEIGHT as i32 >= self.sheet.height() as i32
            {
                // Ignore a point outside the window.
                continue;
            }

            let c = ((color.r() as u32) << 16) + ((color.g() as u32) << 8) + (color.b() as u32);
            self.draw_point(c, point.x as i64, point.y as i64)?;
        }

        Ok(())
    }
}

impl Window {
    pub fn new(name: String, color: u32, x: i64, y: i64, width: i64, height: i64) -> Result<Self> {
        let mut sheet = Sheet::new(Rect::new(x, y, width, height).unwrap());

        bitmap_draw_rect(sheet.bitmap(), color, 0, 0, width, height);

        let mut window = Self {
            name,
            _background_color: color,
            sheet,
            _active: true,
        };

        window.init_titlebar().expect("failed to init titlebar");

        Ok(window)
    }

    fn init_titlebar(&mut self) -> Result<()> {
        let width = self.sheet.width();

        bitmap_draw_rect(self.sheet.bitmap(), DARKBLUE, 0, 0, width, TITLE_BAR_HEIGHT);
        bitmap_draw_string_no_bg(self.sheet.bitmap(), WHITE, 5, 3, &self.name);

        // close button
        bitmap_draw_rect(
            self.sheet.bitmap(),
            GREY,
            width - (6 + BUTTON_SIZE),
            4,
            BUTTON_SIZE,
            BUTTON_SIZE,
        );

        // white high light for button
        bitmap_draw_line(
            self.sheet.bitmap(),
            WHITE,
            width - (6 + BUTTON_SIZE),
            4,
            width - 6,
            4,
        );
        bitmap_draw_line(
            self.sheet.bitmap(),
            WHITE,
            width - (6 + BUTTON_SIZE),
            4,
            width - (6 + BUTTON_SIZE),
            4 + BUTTON_SIZE,
        );

        // shadow for button
        bitmap_draw_line(
            self.sheet.bitmap(),
            DARKGREY,
            width - (6 + BUTTON_SIZE),
            4 + BUTTON_SIZE,
            width - 6,
            4 + BUTTON_SIZE,
        );
        bitmap_draw_line(
            self.sheet.bitmap(),
            DARKGREY,
            width - 6,
            4,
            width - 6,
            4 + BUTTON_SIZE,
        );

        // x
        bitmap_draw_line(
            self.sheet.bitmap(),
            BLACK,
            width - (6 + BUTTON_SIZE) + 4,
            8,
            width - (6 + BUTTON_SIZE) + 12,
            16,
        );
        bitmap_draw_line(
            self.sheet.bitmap(),
            BLACK,
            width - (6 + BUTTON_SIZE) + 4,
            16,
            width - (6 + BUTTON_SIZE) + 12,
            8,
        );

        Ok(())
    }

    // TODO: implement these APIs
    pub fn move_position(&self) {}

    pub fn flush_area(&self, rect: Rect) {
        self.sheet.flush_area(rect);
    }

    pub fn flush(&self) {
        self.sheet.flush();
    }

    pub fn fill_rect(
        &mut self,
        color: u32,
        px: i64,
        py: i64,
        width: i64,
        height: i64,
    ) -> Result<()> {
        if px < 0
            || px + width > self.sheet.width()
            || py < 0
            || py + height + TITLE_BAR_HEIGHT > self.sheet.height()
        {
            return Err(Error::Failed("fill_rect: out of range"));
        }

        bitmap_draw_rect(
            self.sheet.bitmap(),
            color,
            px,
            py + TITLE_BAR_HEIGHT,
            width,
            height,
        );
        Ok(())
    }

    pub fn draw_line(&mut self, color: u32, x0: i64, y0: i64, x1: i64, y1: i64) -> Result<()> {
        if min(x0, x1) < 0
            || min(y0, y1) < 0
            || max(x0, x1) > self.sheet.width()
            || max(y0, y1) + TITLE_BAR_HEIGHT > self.sheet.height()
        {
            return Err(Error::Failed("out of range"));
        }

        bitmap_draw_line(
            self.sheet.bitmap(),
            color,
            x1,
            y1 + TITLE_BAR_HEIGHT,
            x0,
            y0 + TITLE_BAR_HEIGHT,
        );
        Ok(())
    }

    pub fn draw_string(
        &mut self,
        color: u32,
        x: i64,
        y: i64,
        s: &str,
        size: StringSize,
        underline: bool,
    ) -> Result<()> {
        if x < 0 || x > self.sheet.width() || y < 0 || (y + TITLE_BAR_HEIGHT) > self.sheet.height()
        {
            return Err(Error::Failed("draw_string: out of range"));
        }

        match size {
            StringSize::Medium => {
                if underline {
                    bitmap_draw_string_no_bg_with_underline(
                        self.sheet.bitmap(),
                        color,
                        x,
                        y + TITLE_BAR_HEIGHT,
                        s,
                    );
                } else {
                    bitmap_draw_string_no_bg(
                        self.sheet.bitmap(),
                        color,
                        x,
                        y + TITLE_BAR_HEIGHT,
                        s,
                    );
                }
            }
            StringSize::Large => {
                // TODO: support underline
                /*
                graphics::draw_string_2x(
                    color,
                    self.sheet.x() + x,
                    self.sheet.y() + y + TITLE_BAR_HEIGHT,
                    s,
                )?;
                */
                bitmap_draw_string_no_bg(self.sheet.bitmap(), color, x, y + TITLE_BAR_HEIGHT, s);
            }
            StringSize::XLarge => {
                // TODO: support underline
                /*
                graphics::draw_string_3x(
                    color,
                    self.sheet.x() + x,
                    self.sheet.y() + y + TITLE_BAR_HEIGHT,
                    s,
                )?;
                */
                bitmap_draw_string_no_bg(self.sheet.bitmap(), color, x, y + TITLE_BAR_HEIGHT, s);
            }
        }
        Ok(())
    }

    pub fn draw_point(&mut self, color: u32, x: i64, y: i64) -> Result<()> {
        if x < 0 || x > self.sheet.width() || y < 0 || (y + TITLE_BAR_HEIGHT) > self.sheet.height()
        {
            return Err(Error::Failed("draw_point: out of range"));
        }

        bitmap_draw_point(self.sheet.bitmap(), color, x, y + TITLE_BAR_HEIGHT);
        Ok(())
    }
}
