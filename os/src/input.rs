extern crate alloc;

use crate::boot_info::BootInfo;
use crate::command;
use crate::debug_exit;
use crate::efi::fs::EfiFileName;
use crate::error;
use crate::error::Error;
use crate::executor::yield_execution;
use crate::executor::Executor;
use crate::executor::Task;
use crate::executor::TimeoutFuture;
use crate::graphics::draw_point;
use crate::graphics::draw_rect;
use crate::graphics::Bitmap;
use crate::graphics::BitmapBuffer;
use crate::info;
use crate::loader::Elf;
use crate::mutex::Mutex;
use crate::net::manager::network_manager_thread;
use crate::print;
use crate::serial::SerialPort;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::string::String;
use core::str::FromStr;

#[derive(Debug, PartialEq, Eq)]
pub enum KeyEvent {
    None,
    Char(char),
    Enter,
}

impl KeyEvent {
    pub fn to_char(&self) -> Option<char> {
        match self {
            KeyEvent::Char(c) => Some(*c),
            KeyEvent::Enter => Some('\n'),
            _ => None,
        }
    }
}

pub struct MouseButtonState {
    pub l: bool,
    pub c: bool,
    pub r: bool,
}

pub struct InputManager {
    input_queue: Mutex<VecDeque<char>>,
    cursor_queue: Mutex<VecDeque<(f32, f32, MouseButtonState)>>,
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
    pub fn push_cursor_input_absolute(&self, cx: f32, cy: f32, b: MouseButtonState) {
        self.cursor_queue.lock().push_back((cx, cy, b))
    }
    pub fn pop_cursor_input_absolute(&self) -> Option<(f32, f32, MouseButtonState)> {
        self.cursor_queue.lock().pop_front()
    }
}
static INPUT_MANAGER: Mutex<Option<Rc<InputManager>>> = Mutex::new(None, "INPUT_MANAGER");

pub fn enqueue_input_tasks(executor: &mut Executor) {
    let serial_task = async {
        let sp = SerialPort::default();
        loop {
            if let Some(c) = sp.try_read() {
                if let Some(c) = char::from_u32(c as u32) {
                    let c = if c == '\r' { '\n' } else { c };
                    InputManager::take().push_input(c);
                }
            }
            TimeoutFuture::new_ms(20).await;
            yield_execution().await;
        }
    };
    let console_task = async {
        info!("console_task has started");
        let boot_info = BootInfo::take();
        let root_files = boot_info.root_files();
        let root_files: alloc::vec::Vec<&crate::boot_info::File> =
            root_files.iter().filter_map(|e| e.as_ref()).collect();
        let init_app = EfiFileName::from_str("init.txt")?;
        let init_app = root_files.iter().find(|&e| e.name() == &init_app);
        if let Some(init_app) = init_app {
            let init_app = String::from_utf8_lossy(init_app.data());
            let init_app = init_app.trim();
            let init_app = EfiFileName::from_str(init_app)?;
            let elf = root_files.iter().find(|&e| e.name() == &init_app);
            if let Some(elf) = elf {
                let elf = Elf::parse(elf)?;
                let app = elf.load()?;
                app.exec().await?;
                debug_exit::exit_qemu(debug_exit::QemuExitCode::Success);
            } else {
                return Err(Error::Failed("Init app file not found"));
            }
        }

        let mut s = String::new();
        loop {
            if let Some(c) = InputManager::take().pop_input() {
                if c == '\r' || c == '\n' {
                    if let Err(e) = command::run(&s).await {
                        error!("{e:?}");
                    };
                    s.clear();
                }
                match c {
                    'a'..='z' | 'A'..='Z' | '0'..='9' | ' ' | '.' => {
                        print!("{c}");
                        s.push(c);
                    }
                    '\x7f' | '\x08' => {
                        print!("{0} {0}", 0x08 as char);
                        s.pop();
                    }
                    _ => {
                        // Do nothing
                    }
                }
            }
            TimeoutFuture::new_ms(20).await;
            yield_execution().await;
        }
    };
    let mouse_cursor_task = async {
        const CURSOR_SIZE: i64 = 8;
        let mut cursor_bitmap = BitmapBuffer::new(CURSOR_SIZE, CURSOR_SIZE, CURSOR_SIZE);
        for y in 0..CURSOR_SIZE {
            for x in 0..(CURSOR_SIZE - y) {
                draw_point(&mut cursor_bitmap, 0x00ff00, x, y).expect("Failed to paint cursor");
            }
        }
        let mut vram = BootInfo::take().vram();

        // graphics::draw_bmp_clipped(&mut vram, &cursor_bitmap, 100, 100)?;

        let iw = vram.width();
        let ih = vram.height();
        let w = iw as f32;
        let h = ih as f32;
        loop {
            if let Some((px, py, b)) = InputManager::take().pop_cursor_input_absolute() {
                let px = (px * w) as i64;
                let py = (py * h) as i64;
                let px = px.clamp(0, iw - 1);
                let py = py.clamp(0, ih - 1);
                let color = (b.l as u32) * 0xff0000;
                let color = !color;

                draw_rect(&mut vram, color, px, py, 1, 1)?;
            }
            TimeoutFuture::new_ms(15).await;
            yield_execution().await;
        }
    };
    executor.spawn(Task::new(serial_task));
    executor.spawn(Task::new(console_task));
    executor.spawn(Task::new(mouse_cursor_task));
    executor.spawn(Task::new(async { network_manager_thread().await }));
}
