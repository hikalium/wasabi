extern crate alloc;

use core::time::Duration;

use crate::executor::sleep;
use crate::graphics::draw_point;
use crate::gui::GLOBAL_VRAM;
use crate::mutex::Mutex;
use crate::result::Result;
use alloc::collections::VecDeque;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct MouseButtonState(u64);
pub const MOUSE_BUTTON_L: u64 = 1 << 0;
pub const MOUSE_BUTTON_C: u64 = 1 << 1;
pub const MOUSE_BUTTON_R: u64 = 1 << 2;
impl MouseButtonState {
    pub const fn from_lrc(l: bool, r: bool, c: bool) -> Self {
        MouseButtonState(
            MOUSE_BUTTON_L * l as u64
                + MOUSE_BUTTON_C * c as u64
                + MOUSE_BUTTON_R * r as u64,
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
// Unit: px
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct PointerPosition {
    pub x: i64,
    pub y: i64,
}
impl PointerPosition {
    pub const fn from_xy(x: i64, y: i64) -> Self {
        Self { x, y }
    }
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct MouseEvent {
    pub button: MouseButtonState,
    pub position: PointerPosition,
}

pub static GLOBAL_INPUT_MANAGER: InputManager = InputManager::new();

pub struct InputManager {
    mouse_events: Mutex<VecDeque<MouseEvent>>,
    current_mouse_state: Mutex<MouseEvent>,
}
impl InputManager {
    const fn new() -> Self {
        Self {
            mouse_events: Mutex::new(VecDeque::new()),
            current_mouse_state: Mutex::new(MouseEvent {
                button: MouseButtonState::from_lrc(false, false, false),
                position: PointerPosition::from_xy(0, 0),
            }),
        }
    }
    pub fn push_mouse_event(&self, e: MouseEvent) {
        self.mouse_events.lock().push_back(e)
    }
    pub fn pop_mouse_event(&self) -> Option<MouseEvent> {
        self.mouse_events.lock().pop_front()
    }
    pub fn current_mouse_state(&self) -> MouseEvent {
        *self.current_mouse_state.lock()
    }
}

pub async fn input_task() -> Result<()> {
    loop {
        if let Some(e) = GLOBAL_INPUT_MANAGER.pop_mouse_event() {
            let _ = draw_point(
                &mut *GLOBAL_VRAM.lock(),
                0x00ff00,
                e.position.x,
                e.position.y,
            );
            *GLOBAL_INPUT_MANAGER.current_mouse_state.lock() = e;
        }
        sleep(Duration::from_millis(10)).await;
    }
}
