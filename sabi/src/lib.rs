//! # System-Application Binary Interface
//!
//! All types defined here should have repr(C)
//! and all the members should be public.
//! c.f. <https://anssi-fr.github.io/rust-guide/07_ffi.html>

#![no_std]

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct MouseButtonState(u64);
pub const MOUSE_BUTTON_L: u64 = 1 << 0;
pub const MOUSE_BUTTON_C: u64 = 1 << 1;
pub const MOUSE_BUTTON_R: u64 = 1 << 2;
impl MouseButtonState {
    pub fn from_lcr(l: bool, r: bool, c: bool) -> Self {
        MouseButtonState(
            MOUSE_BUTTON_L * l as u64 + MOUSE_BUTTON_C * c as u64 + MOUSE_BUTTON_R * r as u64,
        )
    }
    pub fn l(self) -> bool {
        self.0 & MOUSE_BUTTON_L != 0
    }
    pub fn c(self) -> bool {
        self.0 & MOUSE_BUTTON_C != 0
    }
    pub fn r(self) -> bool {
        self.0 & MOUSE_BUTTON_R != 0
    }
}

// Origin (0, 0) is at top-left of the virtual 2D screen.
// Moving right or down increases the value on each axis.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct PointerPosition {
    pub x: i64,
    pub y: i64,
}
impl PointerPosition {
    pub fn from_xy(x: i64, y: i64) -> Self {
        Self { x, y }
    }
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct MouseEvent {
    pub button: MouseButtonState,
    pub position: PointerPosition,
}
