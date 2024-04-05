extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::graphics;
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
#[derive(Clone, Debug)]
pub struct Window {
    name: String,
    _background_color: u32,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
    _active: bool,
}

/// https://docs.rs/embedded-graphics/latest/embedded_graphics/geometry/trait.OriginDimensions.html
impl OriginDimensions for Window {
    fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
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
            if point.x >= self.width as i32 || point.y >= self.height as i32 {
                // Ignore a point outside the window.
                continue;
            }

            let c = ((color.r() as u32) << 16) + ((color.g() as u32) << 8) + (color.b() as u32);
            graphics::draw_point(c, point.x as i64, point.y as i64)?;
        }

        Ok(())
    }
}

impl Window {
    pub fn new(name: String, color: u32, x: i64, y: i64, width: i64, height: i64) -> Result<Self> {
        graphics::fill_rect(color, x, y, width, height)?;

        let window = Self {
            name,
            _background_color: color,
            x,
            y,
            width,
            height,
            _active: true,
        };

        window.init_titlebar().expect("failed to init titlebar");

        Ok(window)
    }

    fn init_titlebar(&self) -> Result<()> {
        graphics::fill_rect(DARKBLUE, self.x, self.y, self.width, TITLE_BAR_HEIGHT)?;
        graphics::draw_string(WHITE, self.x + 5, self.y + 3, &self.name)?;

        // close button
        graphics::fill_rect(
            GREY,
            self.x + self.width - (6 + BUTTON_SIZE),
            self.y + 4,
            BUTTON_SIZE,
            BUTTON_SIZE,
        )?;

        // white high light for button
        graphics::draw_line(
            WHITE,
            self.x + self.width - (6 + BUTTON_SIZE),
            self.y + 4,
            self.x + self.width - 6,
            self.y + 4,
        )?;
        graphics::draw_line(
            WHITE,
            self.x + self.width - (6 + BUTTON_SIZE),
            self.y + 4,
            self.x + self.width - (6 + BUTTON_SIZE),
            self.y + 4 + BUTTON_SIZE,
        )?;

        // shadow for button
        graphics::draw_line(
            DARKGREY,
            self.x + self.width - (6 + BUTTON_SIZE),
            self.y + 4 + BUTTON_SIZE,
            self.x + self.width - 6,
            self.y + 4 + BUTTON_SIZE,
        )?;
        graphics::draw_line(
            DARKGREY,
            self.x + self.width - 6,
            self.y + 4,
            self.x + self.width - 6,
            self.y + 4 + BUTTON_SIZE,
        )?;

        // x
        graphics::draw_line(
            BLACK,
            self.x + self.width - (6 + BUTTON_SIZE) + 4,
            self.y + 8,
            self.x + self.width - (6 + BUTTON_SIZE) + 12,
            self.y + 16,
        )?;
        graphics::draw_line(
            BLACK,
            self.x + self.width - (6 + BUTTON_SIZE) + 4,
            self.y + 16,
            self.x + self.width - (6 + BUTTON_SIZE) + 12,
            self.y + 8,
        )?;

        Ok(())
    }

    // TODO: implement these APIs
    pub fn move_position(&self) {}
    pub fn flush(&self) {}

    pub fn fill_rect(&self, color: u32, px: i64, py: i64, width: i64, height: i64) -> Result<()> {
        if px < 0 || px + width > self.width || py < 0 || py + height > self.height {
            return Err(Error::Failed("fill_rect: out of range"));
        }

        graphics::fill_rect(
            color,
            self.x + px,
            self.y + py + TITLE_BAR_HEIGHT,
            width,
            height,
        )?;
        Ok(())
    }

    pub fn draw_line(&self, color: u32, x0: i64, y0: i64, x1: i64, y1: i64) -> Result<()> {
        if min(x0, x1) < 0
            || min(y0, y1) < 0
            || max(x0, x1) > self.width
            || max(y0, y1) > self.height
        {
            return Err(Error::Failed("out of range"));
        }

        graphics::draw_line(
            color,
            self.x + x1,
            self.y + y1 + TITLE_BAR_HEIGHT,
            self.x + x0,
            self.y + y0 + TITLE_BAR_HEIGHT,
        )?;
        Ok(())
    }

    pub fn draw_string(
        &self,
        color: u32,
        x: i64,
        y: i64,
        s: &str,
        size: StringSize,
        underline: bool,
    ) -> Result<()> {
        if x < 0 || x > self.width || y < 0 || y > self.height {
            return Err(Error::Failed("draw_string: out of range"));
        }

        match size {
            StringSize::Medium => {
                if underline {
                    graphics::draw_string_with_underline(
                        color,
                        self.x + x,
                        self.y + y + TITLE_BAR_HEIGHT,
                        s,
                    )?;
                } else {
                    graphics::draw_string(color, self.x + x, self.y + y + TITLE_BAR_HEIGHT, s)?;
                }
            }
            StringSize::Large => {
                // TODO: support underline
                graphics::draw_string_2x(color, self.x + x, self.y + y + TITLE_BAR_HEIGHT, s)?;
            }
            StringSize::XLarge => {
                // TODO: support underline
                graphics::draw_string_3x(color, self.x + x, self.y + y + TITLE_BAR_HEIGHT, s)?;
            }
        }
        Ok(())
    }
}
