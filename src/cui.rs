extern crate alloc;

use crate::acpi::RebootParams;
use crate::arp::ArpPacket;
use crate::error;
use crate::executor::sleep;
use crate::executor::spawn_global;
use crate::font::get_glyph_width;
use crate::graphics::draw_str_fg;
use crate::graphics::fill_rect;
use crate::hpet::global_timestamp;
use crate::icmp;
use crate::ime::InputEditResult;
use crate::ime::InputMethodEditor;
use crate::info;
use crate::init::EFI_MEMORY_MAP;
use crate::init::REBOOT_PARAMS;
use crate::ip::IpV4Addr;
use crate::keyboard::KeyEvent;
use crate::nic;
use crate::nic::PingPending;
use crate::print;
use crate::println;
use crate::result::Result;
use crate::slice::Sliceable;
use crate::tablet::set_debug_mouse;
use crate::warn;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::swap;
use core::ptr::write_volatile;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use core::time::Duration;

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

pub struct Console {
    prev_cmd: Option<String>,
    input_buf: String,
    prev_input_was_cr: bool,
    ctrl_is_pressed: bool,
    ime: InputMethodEditor,
}
impl Default for Console {
    fn default() -> Self {
        let mut ime = InputMethodEditor::default();
        ime.init_romaji_map();
        Self {
            prev_cmd: None,
            input_buf: String::new(),
            prev_input_was_cr: false,
            ctrl_is_pressed: false,
            ime,
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
    // Echo a literal char (or apply a Backspace) with no IME, keeping
    // input_buf and the on-screen line in step.
    fn input_plain(&mut self, c: char) {
        if c == '\x08' {
            if let Some(prev_char) = self.input_buf.pop() {
                if get_glyph_width(prev_char) == 16 {
                    print!("\x08");
                    print!("\x08");
                } else {
                    print!("\x08");
                }
            }
        } else {
            self.input_buf.push(c);
            print!("{c}");
        }
    }
    // Redraw after the IME rewrote the line: back over the changed tail
    // of the old text, draw the new tail, then blank any leftover cells
    // when the new tail is shorter (a wide glyph is two cells).
    fn render_ime_line(&mut self, new: &str) {
        let cols = |s: &str, skip: usize| -> usize {
            s.chars()
                .skip(skip)
                .map(|ch| if get_glyph_width(ch) == 16 { 2 } else { 1 })
                .sum()
        };
        let common = self
            .input_buf
            .chars()
            .zip(new.chars())
            .take_while(|(a, b)| a == b)
            .count();
        let old_cols = cols(&self.input_buf, common);
        for _ in 0..old_cols {
            print!("\x08");
        }
        for ch in new.chars().skip(common) {
            print!("{ch}");
        }
        self.input_buf = new.into();
    }
    pub fn handle_key_down(&mut self, e: KeyEvent) {
        let e = match self.normalize_newline(e) {
            Some(e) => e,
            None => return,
        };
        match e {
            KeyEvent::Char(c) => {
                if c == ' ' && self.ctrl_is_pressed {
                    let enable = !is_ime_enabled();
                    set_ime_enabled(enable);
                    // Align the IME's pending text with the line already
                    // on screen so the next keystroke redraws correctly.
                    if enable {
                        self.ime.set_pending(self.input_buf.clone());
                    }
                    return;
                }
                if is_ime_enabled() {
                    match self.ime.send_key_down(KeyEvent::Char(c)) {
                        InputEditResult::UpdatePendingString(s) => {
                            self.render_ime_line(&s)
                        }
                        InputEditResult::PassThrough => self.input_plain(c),
                    }
                } else {
                    self.input_plain(c);
                }
            }
            KeyEvent::CursorUp => {
                if let Some(prev_cmd) = self.prev_cmd.as_mut() {
                    swap(prev_cmd, &mut self.input_buf);
                    print!("\n{}", self.input_buf);
                }
                // Recalled line replaces the buffer; resync the IME.
                self.ime.set_pending(self.input_buf.clone());
            }
            KeyEvent::Enter => {
                println!();
                if let Err(e) = run_cmd(&self.input_buf) {
                    error!("{e}: {}", self.input_buf)
                }
                let mut prev_cmd = String::new();
                swap(&mut prev_cmd, &mut self.input_buf);
                self.prev_cmd = Some(prev_cmd);
                self.ime.set_pending(String::new());
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
            "ping" => run_cmd_ping(&args),
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

fn parse_ipv4(s: &str) -> Result<IpV4Addr> {
    let mut octets = [0u8; 4];
    let mut i = 0;
    for part in s.split('.') {
        if i >= 4 {
            return Err("IP: too many octets");
        }
        octets[i] = part.parse().map_err(|_| "IP: bad octet")?;
        i += 1;
    }
    if i != 4 {
        return Err("IP: expected 4 octets");
    }
    Ok(IpV4Addr::new(octets))
}

const PING_PAYLOAD_LEN: usize = 32;
const PING_REPLY_TIMEOUT: Duration = Duration::from_millis(1000);
const PING_ARP_WAIT: Duration = Duration::from_millis(200);

async fn ping_once(target: IpV4Addr, seq: u16) -> Result<()> {
    let our_mac = nic::our_mac().ok_or("NCM not ready (no MAC)")?;

    // Route to the destination directly when it is on our subnet,
    // otherwise hand the frame to the DHCP-learned default router. Only
    // the link-layer next hop changes; the ICMP packet still targets
    // `target`. Resolve that next hop's MAC, prodding the network with
    // an ARP request if it is not cached yet.
    let next_hop = nic::next_hop(target);
    let dst_mac = match nic::arp_lookup(next_hop) {
        Some(m) => m,
        None => {
            nic::enqueue_tx_frame(
                ArpPacket::request(our_mac, nic::our_ip(), next_hop)
                    .as_slice()
                    .to_vec(),
            );
            sleep(PING_ARP_WAIT).await;
            nic::arp_lookup(next_hop).ok_or("ARP unresolved")?
        }
    };

    let id: u16 = 0x1d10;
    let payload = [0xa5u8; PING_PAYLOAD_LEN];
    let frame = icmp::echo_request_frame(
        our_mac,
        nic::our_ip(),
        dst_mac,
        target,
        id,
        seq,
        &payload,
    );
    let sent_at = global_timestamp();
    *nic::PING_PENDING.lock() = Some(PingPending {
        id,
        seq,
        sent_at,
        reply_rtt: None,
        reply_src: None,
    });
    nic::enqueue_tx_frame(frame);

    let deadline = sent_at + PING_REPLY_TIMEOUT;
    loop {
        // Read out under a short-lived guard so the second lock below
        // doesn't deadlock against an `if let` temporary.
        let reply = nic::PING_PENDING
            .lock()
            .as_ref()
            .and_then(|p| Some((p.reply_rtt?, p.reply_src?)));
        if let Some((rtt, src)) = reply {
            let us = rtt.as_micros();
            println!(
                "{} bytes from {}: icmp_seq={} time={}.{:03} ms",
                PING_PAYLOAD_LEN + 8,
                src,
                seq,
                us / 1000,
                us % 1000,
            );
            *nic::PING_PENDING.lock() = None;
            return Ok(());
        }
        if global_timestamp() >= deadline {
            *nic::PING_PENDING.lock() = None;
            println!("Request timeout for icmp_seq={seq}");
            return Ok(());
        }
        sleep(Duration::from_millis(10)).await;
    }
}

async fn ping_task(target: IpV4Addr, count: u16) -> Result<()> {
    println!("PING {target}: {PING_PAYLOAD_LEN} data bytes");
    for seq in 1..=count {
        if let Err(e) = ping_once(target, seq).await {
            error!("ping: {e}");
            return Ok(());
        }
        if seq != count {
            sleep(Duration::from_millis(1000)).await;
        }
    }
    Ok(())
}

pub fn run_cmd_ping(args: &[&str]) -> Result<()> {
    let target = match args.get(1) {
        Some(s) if !s.is_empty() => parse_ipv4(s)?,
        _ => {
            info!("Usage: ping <ipv4> [count]");
            return Ok(());
        }
    };
    let count: u16 = match args.get(2) {
        Some(s) if !s.is_empty() => s.parse().map_err(|_| "ping: bad count")?,
        _ => 4,
    };
    spawn_global(ping_task(target, count));
    Ok(())
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
