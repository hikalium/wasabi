extern crate alloc;

use crate::acpi::RebootParams;
use crate::error;
use crate::executor::sleep;
use crate::executor::spawn_global;
use crate::executor::yield_execution;
use crate::graphics::draw_button;
use crate::graphics::fill_rect;
use crate::graphics::Rect;
use crate::gui::global_vram_resolutions;
use crate::gui::GLOBAL_VRAM;
use crate::hpet::global_timestamp;
use crate::info;
use crate::init::EFI_MEMORY_MAP;
use crate::init::REBOOT_PARAMS;
use crate::input::MouseEvent;
use crate::input::PointerPosition;
use crate::input::GLOBAL_INPUT_MANAGER;
use crate::keyboard::KeyEvent;
use crate::print;
use crate::println;
use crate::result::Result;
use crate::tablet::set_debug_mouse;
use crate::warn;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::swap;
use core::ptr::write_volatile;
use core::time::Duration;

#[derive(Default)]
pub struct Console {
    prev_cmd: Option<String>,
    input_buf: String,
}
impl Console {
    pub fn handle_key_down(&mut self, e: KeyEvent) {
        match e {
            KeyEvent::Char('\x08') => {
                self.input_buf.pop();
                print!("\x08");
            }
            KeyEvent::Char(c) => {
                self.input_buf.push(c);
                print!("{c}");
            }
            KeyEvent::CursorUp => {
                if let Some(prev_cmd) = self.prev_cmd.as_mut() {
                    swap(prev_cmd, &mut self.input_buf);
                    print!("\n{}", self.input_buf);
                }
            }
            KeyEvent::Enter => {
                println!();
                if let Err(e) = run_cmd(&self.input_buf) {
                    error!("{e}: {}", self.input_buf)
                }
                let mut prev_cmd = String::new();
                swap(&mut prev_cmd, &mut self.input_buf);
                self.prev_cmd = Some(prev_cmd);
            }
            e => warn!("Unhandled input: {e:?}"),
        }
    }
}

pub fn run_cmd_debug(args: &[&str]) -> Result<()> {
    if "mouse" == *args.get(1).unwrap_or(&"") {
        match *args.get(2).unwrap_or(&"") {
            "on" => {
                set_debug_mouse(true);
                info!("mouse debug is on");
                return Ok(());
            }
            "off" => {
                set_debug_mouse(false);
                info!("mouse debug is off");
                return Ok(());
            }
            _ => error!("Expected on or off"),
        };
    }
    info!("Usage:");
    info!("- debug mouse on|off");
    Ok(())
}

pub fn run_cmd_show(args: &[&str]) -> Result<()> {
    if "mmap" == *args.get(1).unwrap_or(&"") {
        if let Some(mmap) = EFI_MEMORY_MAP.lock().as_ref() {
            for e in mmap.iter() {
                println!("{e:?}");
            }
        } else {
            println!("EFI_MEMORY_MAP is not set")
        }
    } else {
        info!("Usage:");
        info!("- show mmap");
    }
    Ok(())
}

pub fn run_cmd_reboot(_args: &[&str]) -> Result<()> {
    let params = (*REBOOT_PARAMS.lock())
        .as_ref()
        .ok_or("RESET_PARAMS not set so can't reboot via ACPI")?
        .clone();
    info!("Using params: {params:?}");
    info!("Rebooting...");
    match params {
        RebootParams::Memory { addr, value } => unsafe {
            write_volatile(addr as *mut u8, value)
        },
        RebootParams::Io { addr, value } => {
            crate::x86::write_io_port_u8(addr, value)
        }
    }
    Ok(())
}

pub fn run_cmd(cmdline: &str) -> Result<()> {
    let args = cmdline.trim();
    let args: Vec<&str> = args.split(' ').collect();
    if let Some(&cmd) = args.first() {
        match cmd {
            "time" => {
                println!("{:?}", global_timestamp());
                Ok(())
            }
            "debug" => run_cmd_debug(&args),
            "show" => run_cmd_show(&args),
            "demo" => run_cmd_demo(&args),
            "reboot" => run_cmd_reboot(&args),
            "hello" => {
                println!("こんにちは");
                Ok(())
            }
            "" => Ok(()),
            _ => Err("Unknown command"),
        }
    } else {
        Ok(())
    }
}

async fn demo_mouse_event_inject_task() -> Result<()> {
    let (w, h) = global_vram_resolutions();
    let xrange = 0..w;
    let yrange = 0..h;
    let mut x = 0;
    let mut y = 0;
    let mut dx = 8;
    let mut dy = 8;
    for _ in 0..1000 {
        x += dx;
        y += dy;
        if !xrange.contains(&x) {
            dx = -dx;
            x += 2 * dx;
        }
        if !yrange.contains(&y) {
            dy = -dy;
            y += 2 * dy;
        }
        GLOBAL_INPUT_MANAGER.push_mouse_event(MouseEvent {
            position: PointerPosition::from_xy(x, y),
            ..Default::default()
        });
        sleep(Duration::from_millis(10)).await;
    }
    Ok(())
}

fn is_rect_pressed(rect: &Rect) -> bool {
    let e = GLOBAL_INPUT_MANAGER.current_mouse_state();

    e.button.l() && rect.contains_point(e.position.x, e.position.y)
}

async fn demo_button_task() -> Result<()> {
    let (vw, vh) = global_vram_resolutions();
    let button_rect = Rect::new(vw / 2, vh / 2, 128, 32)
        .ok_or("Failed to create button rect")?;
    let mut is_pressed_prev = true;
    let _ = fill_rect(
        &mut *GLOBAL_VRAM.lock(),
        0xcccccc,
        button_rect.x() - 10,
        button_rect.y() - 10,
        button_rect.w() + 20,
        button_rect.h() + 20,
    );
    loop {
        let is_pressed = is_rect_pressed(&button_rect);
        if is_pressed != is_pressed_prev {
            let _ = draw_button(
                &mut *GLOBAL_VRAM.lock(),
                vw / 2,
                vh / 2,
                128,
                32,
                0xcccccc,
                is_pressed,
            );
        }
        yield_execution().await;
        is_pressed_prev = is_pressed;
    }
}

pub fn run_cmd_demo(args: &[&str]) -> Result<()> {
    let subcmd = *args.get(1).unwrap_or(&"");
    match subcmd {
        "mouse" => spawn_global(demo_mouse_event_inject_task()),
        "button" => spawn_global(demo_button_task()),
        _ => {
            info!("Usage:");
            info!("- demo mouse");
            info!("- demo button");
        }
    }
    Ok(())
}
