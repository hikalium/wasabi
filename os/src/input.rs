extern crate alloc;

use crate::mutex::Mutex;
use alloc::collections::VecDeque;
use alloc::rc::Rc;

pub struct InputManager {
    input_queue: Mutex<VecDeque<char>>,
    cursor_queue: Mutex<VecDeque<(f32, f32)>>,
}
impl InputManager {
    fn new() -> Self {
        Self {
            input_queue: Mutex::new(VecDeque::new(), "InputManager.input_queue"),
            cursor_queue: Mutex::new(VecDeque::new(), "InputManager.cursor_queue"),
        }
    }
    pub fn take() -> Rc<Self> {
        let mut instance = INPUT_MANAGER.lock();
        let instance = instance.get_or_insert_with(|| Rc::new(Self::new()));
        instance.clone()
    }
    pub fn push_input(&self, value: char) {
        self.input_queue.lock().push_back(value)
    }
    pub fn pop_input(&self) -> Option<char> {
        self.input_queue.lock().pop_front()
    }

    // x, y: 0f32..1f32, top left origin
    pub fn push_cursor_input_absolute(&self, cx: f32, cy: f32) {
        self.cursor_queue.lock().push_back((cx, cy))
    }
    pub fn pop_cursor_input_absolute(&self) -> Option<(f32, f32)> {
        self.cursor_queue.lock().pop_front()
    }
}
static INPUT_MANAGER: Mutex<Option<Rc<InputManager>>> = Mutex::new(None, "INPUT_MANAGER");
