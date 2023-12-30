extern crate alloc;

use crate::draw_line;
use crate::draw_string;
use crate::draw_string_2x;
use crate::draw_string_3x;
use crate::error::Error;
use crate::error::Result;
use crate::fill_rect;
use alloc::string::String;
use core::cmp::max;
use core::cmp::min;

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

impl Window {
    pub fn new(name: String, color: u32, x: i64, y: i64, width: i64, height: i64) -> Result<Self> {
        fill_rect(color, x, y, width, height)?;

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
        fill_rect(DARKBLUE, self.x, self.y, self.width, TITLE_BAR_HEIGHT)?;
        draw_string(WHITE, self.x + 5, self.y + 3, &self.name)?;

        // close button
        fill_rect(
            GREY,
            self.x + self.width - (6 + BUTTON_SIZE),
            self.y + 4,
            BUTTON_SIZE,
            BUTTON_SIZE,
        )?;

        // white high light for button
        draw_line(
            WHITE,
            self.x + self.width - (6 + BUTTON_SIZE),
            self.y + 4,
            self.x + self.width - 6,
            self.y + 4,
        )?;
        draw_line(
            WHITE,
            self.x + self.width - (6 + BUTTON_SIZE),
            self.y + 4,
            self.x + self.width - (6 + BUTTON_SIZE),
            self.y + 4 + BUTTON_SIZE,
        )?;

        // shadow for button
        draw_line(
            DARKGREY,
            self.x + self.width - (6 + BUTTON_SIZE),
            self.y + 4 + BUTTON_SIZE,
            self.x + self.width - 6,
            self.y + 4 + BUTTON_SIZE,
        )?;
        draw_line(
            DARKGREY,
            self.x + self.width - 6,
            self.y + 4,
            self.x + self.width - 6,
            self.y + 4 + BUTTON_SIZE,
        )?;

        // x
        draw_line(
            BLACK,
            self.x + self.width - (6 + BUTTON_SIZE) + 4,
            self.y + 8,
            self.x + self.width - (6 + BUTTON_SIZE) + 12,
            self.y + 16,
        )?;
        draw_line(
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

        fill_rect(
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

        draw_line(
            color,
            self.x + x1,
            self.y + y1 + TITLE_BAR_HEIGHT,
            self.x + x0,
            self.y + y0 + TITLE_BAR_HEIGHT,
        )?;
        Ok(())
    }

    pub fn draw_string(&self, color: u32, x: i64, y: i64, s: &str, size: StringSize) -> Result<()> {
        if x < 0 || x > self.width || y < 0 || y > self.height {
            return Err(Error::Failed("draw_string: out of range"));
        }

        match size {
            StringSize::Medium => {
                draw_string(color, self.x + x, self.y + y + TITLE_BAR_HEIGHT, s)?;
            }
            StringSize::Large => {
                draw_string_2x(color, self.x + x, self.y + y + TITLE_BAR_HEIGHT, s)?;
            }
            StringSize::XLarge => {
                draw_string_3x(color, self.x + x, self.y + y + TITLE_BAR_HEIGHT, s)?;
            }
        }
        Ok(())
    }
}
