extern crate alloc;

use crate::acpi::RebootParams;
use crate::error;
use crate::font::get_glyph_width;
use crate::graphics::draw_str_fg;
use crate::graphics::fill_rect;
use crate::hpet::global_timestamp;
use crate::ime::InputEditResult;
use crate::ime::InputMethodEditor;
use crate::info;
use crate::init::EFI_MEMORY_MAP;
use crate::init::REBOOT_PARAMS;
use crate::keyboard::KeyEvent;
use crate::print;
use crate::print::cursor_position;
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

// One char of the input line together with how it is drawn: pending
// text is underlined, committed text is not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct StyledChar {
    ch: char,
    underline: bool,
}

// The committed text followed by the pending text, underlined.
fn styled_line(committed: &str, pending: &str) -> Vec<StyledChar> {
    let committed = committed.chars().map(|ch| StyledChar {
        ch,
        underline: false,
    });
    let pending = pending.chars().map(|ch| StyledChar {
        ch,
        underline: true,
    });
    committed.chain(pending).collect()
}

// How many 8-pixel cells c takes on screen, by the same rule as
// BitmapTextWriter (a char missing from the font takes none).
fn glyph_cells(c: char) -> usize {
    (get_glyph_width(c) / 8) as usize
}

// Encodes the redraw from old to new as one string: erase the tail of
// old that differs, then draw the tail of new, switching the underline
// on and off as needed. Empty when nothing changed.
fn encode_line_diff(old: &[StyledChar], new: &[StyledChar]) -> String {
    let common = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    if old.len() == common && new.len() == common {
        return String::new();
    }
    let mut out = String::from("\x1b[24m");
    // Terminals only move the cursor on Backspace, so erase the old
    // tail by overwriting it with spaces, then back up over them.
    let old_cells: usize =
        old[common..].iter().map(|sc| glyph_cells(sc.ch)).sum();
    out.push_str(&"\x08".repeat(old_cells));
    out.push_str(&" ".repeat(old_cells));
    out.push_str(&"\x08".repeat(old_cells));
    let mut underline = false;
    for sc in &new[common..] {
        if sc.underline != underline {
            out.push_str(if sc.underline { "\x1b[4m" } else { "\x1b[24m" });
            underline = sc.underline;
        }
        out.push(sc.ch);
    }
    if underline {
        out.push_str("\x1b[24m");
    }
    out
}

// How many candidates the candidate window shows at once.
const CANDIDATE_WINDOW_SLOTS: usize = 3;

// The page of candidates that the selected one is on, and where the
// selected one is within that page.
fn candidate_page(candidates: &[String], index: usize) -> (&[String], usize) {
    let start = index / CANDIDATE_WINDOW_SLOTS * CANDIDATE_WINDOW_SLOTS;
    let end = (start + CANDIDATE_WINDOW_SLOTS).min(candidates.len());
    (&candidates[start..end], index - start)
}

fn text_width(s: &str) -> i64 {
    s.chars().map(get_glyph_width).sum()
}

pub struct Console {
    prev_cmd: Option<String>,
    input_buf: String,
    prev_input_was_cr: bool,
    ctrl_is_pressed: bool,
    shift_is_pressed: bool,
    ime: InputMethodEditor,
    // Where the candidate window is drawn, while it is shown.
    // (x, y, w, h) in pixels.
    candidate_window: Option<(i64, i64, i64, i64)>,
}
impl Default for Console {
    fn default() -> Self {
        let mut ime = InputMethodEditor::default();
        ime.init_romaji_map();
        ime.init_dict();
        Self {
            prev_cmd: None,
            input_buf: String::new(),
            prev_input_was_cr: false,
            ctrl_is_pressed: false,
            shift_is_pressed: false,
            ime,
            candidate_window: None,
        }
    }
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
    // The input line as it should look on screen right now.
    fn line(&self) -> Vec<StyledChar> {
        styled_line(&self.input_buf, self.ime.pending())
    }
    // Brings the screen from the old line to the current one in a
    // single print, so erase, redraw, and underline changes are not
    // interleaved with other output.
    fn redraw(&self, old: &[StyledChar]) {
        print!("{}", encode_line_diff(old, &self.line()));
    }
    // Edits the committed text directly: append c, or on Backspace
    // remove the last char.
    fn edit_committed(&mut self, c: char) {
        if c == '\x08' {
            self.input_buf.pop();
        } else {
            self.input_buf.push(c);
        }
    }
    // Moves the IME's pending text onto the end of the committed line.
    fn commit_pending(&mut self) {
        let pending = self.ime.take_pending();
        self.input_buf.push_str(&pending);
    }
    // Shows the candidates of the phrase being converted on the line
    // below the pending text, the selected one highlighted, or takes
    // the window down when no conversion is in progress. Like the IME
    // indicator it is drawn straight into VRAM rather than printed, so
    // it stays out of the text stream (terminals do not see it).
    fn update_candidate_window(&mut self) {
        let (cx, cy) = cursor_position();
        let (_, vh) =
            crate::print::get_global_vram_resolutions().unwrap_or((0, 0));
        if let Some((x, y, w, h)) = self.candidate_window.take() {
            crate::print::with_global_vram_buf(|buf| {
                let _ = fill_rect(buf, 0x000000, x, y, w, h);
            });
        }
        let Some((candidates, index)) = self.ime.candidates() else {
            return;
        };
        let (page, selected) = candidate_page(candidates, index);
        // Below the start of the pending text. The line below the
        // bottom one is the top one, as for the text writer.
        let x = (cx - text_width(self.ime.pending())).max(0);
        let y = if cy + 16 < vh { cy + 16 } else { 0 };
        let w = page.iter().map(|c| text_width(c) + 8).sum::<i64>() + 8;
        crate::print::with_global_vram_buf(|buf| {
            let _ = fill_rect(buf, 0x404040, x, y, w, 16);
            let mut cx = x + 8;
            for (i, c) in page.iter().enumerate() {
                let cw = text_width(c);
                let fg = if i == selected {
                    let _ = fill_rect(buf, 0xffffff, cx, y, cw, 16);
                    0x000000
                } else {
                    0xffffff
                };
                draw_str_fg(buf, cx, y, fg, c);
                cx += cw + 8;
            }
        });
        self.candidate_window = Some((x, y, w, 16));
    }
    pub fn handle_key_down(&mut self, e: KeyEvent) {
        let e = match self.normalize_newline(e) {
            Some(e) => e,
            None => return,
        };
        // The `ime off` command can disable the IME while text is still
        // pending; nothing else will ever confirm it, so commit it here.
        if !is_ime_enabled() && !self.ime.pending().is_empty() {
            let old = self.line();
            self.commit_pending();
            self.redraw(&old);
        }
        let old = self.line();
        match e {
            KeyEvent::Char(c) => {
                if c == ' ' && self.ctrl_is_pressed {
                    let enable = !is_ime_enabled();
                    set_ime_enabled(enable);
                    if !enable {
                        self.commit_pending();
                    }
                } else if c == ' '
                    && self.shift_is_pressed
                    && self.ime.step_candidate(-1).is_some()
                {
                    // Shift+Space goes back to the previous candidate.
                } else if is_ime_enabled() {
                    // On UpdatePendingString the IME already holds the
                    // new pending text; the redraw below shows it.
                    let result = self.ime.send_key_down(KeyEvent::Char(c));
                    if result == InputEditResult::PassThrough {
                        self.edit_committed(c);
                    }
                } else {
                    self.edit_committed(c);
                }
            }
            // While candidates are being chosen, the cursor keys move
            // through them: right and down go on, left and up go back.
            KeyEvent::CursorRight | KeyEvent::CursorDown
                if self.ime.candidates().is_some() =>
            {
                self.ime.step_candidate(1);
            }
            KeyEvent::CursorLeft | KeyEvent::CursorUp
                if self.ime.candidates().is_some() =>
            {
                self.ime.step_candidate(-1);
            }
            KeyEvent::CursorUp => {
                if let Some(mut prev_cmd) = self.prev_cmd.take() {
                    // The recalled line replaces the whole line, so
                    // settle the pending text into it first.
                    self.commit_pending();
                    self.redraw(&old);
                    self.update_candidate_window();
                    swap(&mut prev_cmd, &mut self.input_buf);
                    self.prev_cmd = Some(prev_cmd);
                    print!("\n{}", self.input_buf);
                    return;
                }
            }
            KeyEvent::Enter => {
                // Enter confirms pending text first; only a second Enter
                // with nothing pending runs the line as a command.
                if !self.ime.pending().is_empty() {
                    self.commit_pending();
                } else {
                    println!();
                    if let Err(e) = run_cmd(&self.input_buf) {
                        error!("{e}: {}", self.input_buf)
                    }
                    let mut prev_cmd = String::new();
                    swap(&mut prev_cmd, &mut self.input_buf);
                    self.prev_cmd = Some(prev_cmd);
                    print!("> ");
                    return;
                }
            }
            KeyEvent::CtrlLeft => {
                self.ctrl_is_pressed = true;
            }
            KeyEvent::ShiftLeft => {
                self.shift_is_pressed = true;
            }
            e => warn!("Unhandled input: {e:?}"),
        }
        self.redraw(&old);
        self.update_candidate_window();
    }
    pub fn handle_key_up(&mut self, e: KeyEvent) {
        match e {
            KeyEvent::CtrlLeft => self.ctrl_is_pressed = false,
            KeyEvent::ShiftLeft => self.shift_is_pressed = false,
            _ => {}
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

#[cfg(test)]
mod line_edit_tests {
    use super::*;
    use crate::bitmap::BitmapBuffer;
    use crate::graphics::BitmapTextWriter;
    use core::fmt::Write;

    fn console_with_ime() -> Console {
        set_ime_enabled(true);
        Console::default()
    }

    fn type_str(con: &mut Console, s: &str) {
        for c in s.chars() {
            con.handle_key_down(KeyEvent::Char(c));
        }
    }

    fn toggle_ime_with_ctrl_space(con: &mut Console) {
        con.handle_key_down(KeyEvent::CtrlLeft);
        con.handle_key_down(KeyEvent::Char(' '));
        con.handle_key_up(KeyEvent::CtrlLeft);
    }

    #[test_case]
    fn enter_commits_pending_before_running_command() {
        let mut con = console_with_ime();
        type_str(&mut con, "a");
        assert_eq!(con.input_buf, "");
        assert_eq!(con.ime.pending(), "あ");
        con.handle_key_down(KeyEvent::Enter);
        assert_eq!(con.input_buf, "あ");
        assert_eq!(con.ime.pending(), "");
        // Nothing pending: this Enter runs the line as a command.
        con.handle_key_down(KeyEvent::Enter);
        assert_eq!(con.input_buf, "");
        assert_eq!(con.prev_cmd.as_deref(), Some("あ"));
        set_ime_enabled(false);
    }

    #[test_case]
    fn deleting_last_pending_char_keeps_committed_text() {
        let mut con = console_with_ime();
        con.input_buf = String::from("A");
        type_str(&mut con, "a");
        type_str(&mut con, "\x08");
        assert_eq!(con.input_buf, "A");
        assert_eq!(con.ime.pending(), "");
        type_str(&mut con, "\x08");
        assert_eq!(con.input_buf, "");
        set_ime_enabled(false);
    }

    #[test_case]
    fn cursor_up_without_history_preserves_pending() {
        let mut con = console_with_ime();
        type_str(&mut con, "a");
        con.handle_key_down(KeyEvent::CursorUp);
        assert_eq!(con.input_buf, "");
        assert_eq!(con.ime.pending(), "あ");
        set_ime_enabled(false);
    }

    #[test_case]
    fn cursor_up_commits_pending_then_recalls() {
        let mut con = console_with_ime();
        con.prev_cmd = Some(String::from("time"));
        con.input_buf = String::from("A");
        type_str(&mut con, "a");
        con.handle_key_down(KeyEvent::CursorUp);
        assert_eq!(con.input_buf, "time");
        assert_eq!(con.prev_cmd.as_deref(), Some("Aあ"));
        assert_eq!(con.ime.pending(), "");
        set_ime_enabled(false);
    }

    #[test_case]
    fn disabling_ime_commits_pending() {
        // Via Ctrl+Space, which the Console handles itself.
        let mut con = console_with_ime();
        type_str(&mut con, "a");
        toggle_ime_with_ctrl_space(&mut con);
        assert!(!is_ime_enabled());
        assert_eq!(con.input_buf, "あ");
        assert_eq!(con.ime.pending(), "");
        // Via the `ime off` command, which the Console only notices on
        // the next key.
        set_ime_enabled(true);
        type_str(&mut con, "i");
        run_cmd("ime off").unwrap();
        type_str(&mut con, "B");
        assert_eq!(con.input_buf, "あいB");
        assert_eq!(con.ime.pending(), "");
    }

    #[test_case]
    fn enabling_ime_does_not_import_committed_text() {
        set_ime_enabled(false);
        let mut con = Console::default();
        type_str(&mut con, "A");
        toggle_ime_with_ctrl_space(&mut con);
        assert!(is_ime_enabled());
        assert_eq!(con.ime.pending(), "");
        type_str(&mut con, "a");
        assert_eq!(con.input_buf, "A");
        assert_eq!(con.ime.pending(), "あ");
        set_ime_enabled(false);
    }

    #[test_case]
    fn sentence_is_built_from_committed_phrases() {
        let mut con = console_with_ime();
        for phrase in ["kyouno ", "tennkiha ", "amedesu "] {
            type_str(&mut con, phrase);
            con.handle_key_down(KeyEvent::Enter);
        }
        assert_eq!(con.input_buf, "今日の天気は雨です");
        assert_eq!(con.ime.pending(), "");
        set_ime_enabled(false);
    }

    #[test_case]
    fn name_sentence_is_built_from_committed_phrases() {
        let mut con = console_with_ime();
        for phrase in ["watasino ", "namaeha ", "nakanodesu "] {
            type_str(&mut con, phrase);
            con.handle_key_down(KeyEvent::Enter);
        }
        assert_eq!(con.input_buf, "私の名前は中野です");
        assert_eq!(con.ime.pending(), "");
        set_ime_enabled(false);
    }

    #[test_case]
    fn homophone_sentence_is_built_by_choosing_candidates() {
        // Each phrase reads きしゃ; the extra Spaces pick the 1st, 2nd,
        // 3rd, and 4th candidate in turn.
        let mut con = console_with_ime();
        for phrase in ["kisyano ", "kisyaga  ", "kisyade   ", "kisyasita    "] {
            type_str(&mut con, phrase);
            con.handle_key_down(KeyEvent::Enter);
        }
        assert_eq!(con.input_buf, "貴社の記者が汽車で帰社した");
        assert_eq!(con.ime.pending(), "");
        set_ime_enabled(false);
    }

    #[test_case]
    fn shift_space_and_cursor_keys_move_between_candidates() {
        let mut con = console_with_ime();
        type_str(&mut con, "kisya ");
        assert_eq!(con.ime.pending(), "貴社");
        con.handle_key_down(KeyEvent::CursorRight);
        assert_eq!(con.ime.pending(), "記者");
        con.handle_key_down(KeyEvent::CursorDown);
        assert_eq!(con.ime.pending(), "汽車");
        con.handle_key_down(KeyEvent::CursorLeft);
        assert_eq!(con.ime.pending(), "記者");
        // Up moves back through the candidates rather than recalling
        // history while a conversion is in progress.
        con.prev_cmd = Some(String::from("time"));
        con.handle_key_down(KeyEvent::CursorUp);
        assert_eq!(con.ime.pending(), "貴社");
        assert_eq!(con.input_buf, "");
        // Shift+Space goes back too, wrapping around to the last
        // candidate, the katakana.
        con.handle_key_down(KeyEvent::ShiftLeft);
        con.handle_key_down(KeyEvent::Char(' '));
        con.handle_key_up(KeyEvent::ShiftLeft);
        assert_eq!(con.ime.pending(), "キシャ");
        // Plain Space still goes forward.
        type_str(&mut con, " ");
        assert_eq!(con.ime.pending(), "貴社");
        set_ime_enabled(false);
    }

    #[test_case]
    fn candidate_page_holds_three_and_follows_the_selection() {
        let all: Vec<String> = ["a", "b", "c", "d"]
            .iter()
            .map(|s| String::from(*s))
            .collect();
        assert_eq!(candidate_page(&all, 0), (&all[..3], 0));
        assert_eq!(candidate_page(&all, 2), (&all[..3], 2));
        assert_eq!(candidate_page(&all, 3), (&all[3..], 0));
    }

    #[test_case]
    fn candidate_window_is_shown_only_while_converting() {
        let mut con = console_with_ime();
        type_str(&mut con, "ame");
        assert!(con.candidate_window.is_none());
        type_str(&mut con, " ");
        assert!(con.candidate_window.is_some());
        // Backspace cancels the conversion and takes the window down.
        type_str(&mut con, "\x08");
        assert!(con.candidate_window.is_none());
        type_str(&mut con, " ");
        assert!(con.candidate_window.is_some());
        // So does committing.
        con.handle_key_down(KeyEvent::Enter);
        assert!(con.candidate_window.is_none());
        set_ime_enabled(false);
    }

    #[test_case]
    fn unchanged_line_produces_no_output() {
        let line = styled_line("A", "あ");
        assert_eq!(encode_line_diff(&line, &line), "");
        assert_eq!(encode_line_diff(&[], &[]), "");
    }

    #[test_case]
    fn committing_same_text_requires_redraw() {
        // Same chars, but the underline has to go: not a no-op.
        let pending = styled_line("", "今日");
        let committed = styled_line("今日", "");
        assert_ne!(encode_line_diff(&pending, &committed), "");
    }

    #[test_case]
    fn cell_width_matches_bitmap_writer() {
        assert_eq!(glyph_cells('A'), 1);
        assert_eq!(glyph_cells('日'), 2);
        assert_eq!(glyph_cells('\u{3000}'), 2);
        assert_eq!(glyph_cells('ぃ'), 2);
        // U+FFFF is a noncharacter, so no font has a glyph for it.
        assert_eq!(glyph_cells('\u{FFFF}'), 0);
    }

    #[test_case]
    fn shrinking_pending_clears_old_tail() {
        // Two cells of い are backed over, blanked, and backed over
        // again; nothing new is drawn.
        assert_eq!(
            encode_line_diff(&styled_line("", "あい"), &styled_line("", "あ")),
            "\x1b[24m\x08\x08  \x08\x08"
        );
    }

    #[test_case]
    fn commit_redraw_matches_plain_text_pixels() {
        let pending = styled_line("", "今日");
        let committed = styled_line("今日", "");
        let mut redrawn = BitmapTextWriter::new(BitmapBuffer::new(64, 16, 64));
        redrawn.write_str(&encode_line_diff(&[], &pending)).unwrap();
        redrawn
            .write_str(&encode_line_diff(&pending, &committed))
            .unwrap();
        let mut plain = BitmapTextWriter::new(BitmapBuffer::new(64, 16, 64));
        plain.write_str("今日").unwrap();
        assert_eq!(redrawn.buf(), plain.buf());
    }
}
