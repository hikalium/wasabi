extern crate alloc;

use crate::*;
use alloc::string::String;

static WHITE: u32 = 0xffffff;
static DARKBLUE: u32 = 0x00008b;
static GREY: u32 = 0x808080;
static DARKGREY: u32 = 0x5a5a5a;
static BLACK: u32 = 0x000000;

static TITLE_BAR_HEIGHT: i64 = 24;
static BUTTON_SIZE: i64 = 16;

/// Represent a window for one application.
pub struct Window {
    name: String,
    background_color: u32,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
    active: bool,
}

impl Window {
    pub fn new(
        name: String,
        color: u32,
        x: i64,
        y: i64,
        width: i64,
        height: i64,
    ) -> Result<Self, ()> {
        fill_rect(color, x, y, width, height)?;

        let window = Self {
            name,
            background_color: color,
            x,
            y,
            width,
            height,
            active: true,
        };

        window.init_titlebar();

        Ok(window)
    }

    fn init_titlebar(&self) -> Result<(), ()> {
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

    // TODO: implement once a user can move a window via mouse input.
    pub fn r#move(&self) {}

    pub fn draw_string(&self, color: u32, x: i64, y: i64, s: &str) -> Result<(), ()> {
        draw_string(color, self.x + x, self.y + y, s)?;
        Ok(())
    }
}
