extern crate alloc;

use crate::acpi::RebootParams;
use crate::error;
use crate::font::get_glyph_width;
use crate::graphics::draw_str_fg;
use crate::graphics::fill_rect;
use crate::hpet::global_timestamp;
use crate::info;
use crate::init::EFI_MEMORY_MAP;
use crate::init::REBOOT_PARAMS;
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
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

// IME on/off is global state shared by every Console instance (the USB
// keyboard and the remote console each own their own Console), and is
// also driven by the `ime` command which runs outside any Console.
static IS_IME_ENABLED: AtomicBool = AtomicBool::new(false);
pub fn is_ime_enabled() -> bool {
    IS_IME_ENABLED.load(Ordering::Relaxed)
}
pub fn set_ime_enabled(choice: bool) {
    IS_IME_ENABLED.store(choice, Ordering::Relaxed);
    let (vw, vh) =
        crate::print::get_global_vram_resolutions().unwrap_or((0, 0));
    crate::print::with_global_vram_buf(|buf| {
        // Ignore failure (e.g. zero-resolution VRAM under the test harness).
        let _ = fill_rect(buf, 0x000000, vw - 16, vh - 16, 16, 16);
        draw_str_fg(
            buf,
            vw - 16,
            vh - 16,
            0xffffff,
            if choice { "あ" } else { "Aa" },
        );
    });
}

#[derive(Default)]
pub struct Console {
    prev_cmd: Option<String>,
    input_buf: String,
    prev_input_was_cr: bool,
    ctrl_is_pressed: bool,
}
impl Console {
    /// Maps raw newline characters onto [`KeyEvent::Enter`] so that CR,
    /// LF, and CRLF inputs all trigger the command exactly once: some
    /// input sources send CR for the Enter key, some send LF, and some
    /// send the CRLF pair. Returns `None` when the event should be
    /// dropped (the LF right after a CR, i.e. the second half of a
    /// CRLF). The "was the previous input a CR?" state is kept in
    /// `self.prev_input_was_cr`.
    fn normalize_newline(&mut self, e: KeyEvent) -> Option<KeyEvent> {
        let was_cr = self.prev_input_was_cr;
        self.prev_input_was_cr = matches!(e, KeyEvent::Char('\r'));
        match e {
            KeyEvent::Char('\r') => Some(KeyEvent::Enter),
            KeyEvent::Char('\n') if was_cr => None,
            KeyEvent::Char('\n') => Some(KeyEvent::Enter),
            e => Some(e),
        }
    }
    pub fn boin_index(e: char) -> Option<usize> {
        match e {
            'a' => Some(0),
            'i' => Some(1),
            'u' => Some(2),
            'e' => Some(3),
            'o' => Some(4),
            _ => None,
        }
    }
    pub fn shiin_index(e: char) -> Option<usize> {
        match e {
            'k' => Some(0),
            's' => Some(1),
            't' => Some(2),
            'n' => Some(3),
            'h' => Some(4),
            'm' => Some(5),
            'y' => Some(6),
            'r' => Some(7),
            'w' => Some(8),
            // dakuon
            'g' => Some(9),
            'z' => Some(10),
            'd' => Some(11),
            'b' => Some(12),
            // handakuon
            'p' => Some(13),
            _ => None,
        }
    }
    pub fn handle_key_down(&mut self, e: KeyEvent) {
        let e = match self.normalize_newline(e) {
            Some(e) => e,
            None => return,
        };
        match e {
            KeyEvent::Char('\x08') => {
                if let Some(prev_char) = self.input_buf.pop() {
                    let gw = get_glyph_width(prev_char);
                    if gw == 16 {
                        print!("\x08");
                        print!("\x08");
                    } else {
                        print!("\x08");
                    }
                }
            }
            KeyEvent::Char(c) => {
                if c == ' ' && self.ctrl_is_pressed {
                    set_ime_enabled(!is_ime_enabled());
                    return;
                }
                let c = if is_ime_enabled() {
                    let prev1_char = c;
                    let prev1 = Self::boin_index(c);
                    let prev2_char = self.input_buf.chars().last();
                    let prev2 = if !self.input_buf.is_empty() {
                        self.input_buf
                            .chars()
                            .last()
                            .and_then(Self::shiin_index)
                    } else {
                        None
                    };
                    if let (_, '-') = (prev2_char, prev1_char) {
                        self.input_buf.pop();
                        'ー'
                    } else if let (Some('n'), 'n') = (prev2_char, prev1_char) {
                        self.input_buf.pop();
                        print!("\x08");
                        'ん'
                    } else {
                        match (prev2, prev1) {
                            (Some(si), Some(bi)) => {
                                self.input_buf.pop();
                                print!("\x08");
                                [
                                    ['か', 'き', 'く', 'け', 'こ'],
                                    ['さ', 'し', 'す', 'せ', 'そ'],
                                    ['た', 'ち', 'つ', 'て', 'と'],
                                    ['な', 'に', 'ぬ', 'ね', 'の'],
                                    ['は', 'ひ', 'ふ', 'へ', 'ほ'],
                                    ['ま', 'み', 'む', 'め', 'も'],
                                    ['や', '　', 'ゆ', '　', 'よ'],
                                    ['ら', 'り', 'る', 'れ', 'ろ'],
                                    ['わ', '　', '　', '　', 'を'],
                                    ['が', 'ぎ', 'ぐ', 'げ', 'ご'],
                                    ['ざ', 'じ', 'ず', 'ぜ', 'ぞ'],
                                    ['だ', 'ぢ', 'づ', 'で', 'ど'],
                                    ['ば', 'び', 'ぶ', 'べ', 'ぼ'],
                                    ['ぱ', 'ぴ', 'ぷ', 'ぺ', 'ぽ'],
                                ][si][bi]
                            }
                            (_, Some(bi)) => ['あ', 'い', 'う', 'え', 'お'][bi],
                            _ => c,
                        }
                    }
                } else {
                    c
                };
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
                print!("> ");
            }
            KeyEvent::CtrlLeft => {
                self.ctrl_is_pressed = true;
            }
            e => warn!("Unhandled input: {e:?}"),
        }
    }
    pub fn handle_key_up(&mut self, e: KeyEvent) {
        if e == KeyEvent::CtrlLeft {
            self.ctrl_is_pressed = false;
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

pub fn run_cmd_ime(args: &[&str]) -> Result<()> {
    match *args.get(1).unwrap_or(&"") {
        "on" => {
            set_ime_enabled(true);
            info!("ime is on");
            return Ok(());
        }
        "off" => {
            set_ime_enabled(false);
            info!("ime is off");
            return Ok(());
        }
        "" => {
            info!("ime is {}", if is_ime_enabled() { "on" } else { "off" });
            return Ok(());
        }
        _ => error!("Expected on or off"),
    };
    info!("Usage:");
    info!("- ime [on|off]");
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

// Cfg-selected at the consumer's compile time — proc-macros don't
// see the consumer's target_arch in their environment, but `cfg`
// attributes do.
#[cfg(target_arch = "x86_64")]
const TARGET_ARCH: &str = "x86_64";
#[cfg(not(target_arch = "x86_64"))]
const TARGET_ARCH: &str = "unknown";

pub fn run_cmd_uname(_args: &[&str]) -> Result<()> {
    println!(
        "WasabiOS {} ({}) {}",
        env!("CARGO_PKG_VERSION"),
        version_macro::git_hash!(),
        TARGET_ARCH,
    );
    Ok(())
}

#[cfg(test)]
mod uname_tests {
    use super::TARGET_ARCH;

    #[test_case]
    fn target_arch_is_known() {
        assert_ne!(TARGET_ARCH, "unknown");
    }

    #[test_case]
    fn git_hash_is_known() {
        assert_ne!(version_macro::git_hash!(), "unknown");
    }
}

#[cfg(test)]
mod ime_tests {
    use super::is_ime_enabled;
    use super::run_cmd;

    #[test_case]
    fn ime_command_toggles_state() {
        run_cmd("ime off").unwrap();
        assert!(!is_ime_enabled());
        run_cmd("ime on").unwrap();
        assert!(is_ime_enabled());
        run_cmd("ime off").unwrap();
        assert!(!is_ime_enabled());
    }
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
            "ime" => run_cmd_ime(&args),
            "show" => run_cmd_show(&args),
            "reboot" | "r" => run_cmd_reboot(&args),
            "uname" => run_cmd_uname(&args),
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

#[test_case]
fn cr_lf_and_crlf_trigger_enter_exactly_once() {
    let mut con = Console::default();
    // A lone CR triggers Enter.
    assert!(matches!(
        con.normalize_newline(KeyEvent::Char('\r')),
        Some(KeyEvent::Enter)
    ));
    // The LF right after the CR (CRLF) is dropped.
    assert!(matches!(con.normalize_newline(KeyEvent::Char('\n')), None));
    // A lone LF triggers Enter.
    assert!(matches!(
        con.normalize_newline(KeyEvent::Char('\n')),
        Some(KeyEvent::Enter)
    ));
    // Ordinary characters pass through and reset the CR state.
    con.normalize_newline(KeyEvent::Char('\r'));
    assert!(matches!(
        con.normalize_newline(KeyEvent::Char('a')),
        Some(KeyEvent::Char('a'))
    ));
    assert!(matches!(
        con.normalize_newline(KeyEvent::Char('\n')),
        Some(KeyEvent::Enter)
    ));
}
