extern crate alloc;

use crate::acpi::RebootParams;
use crate::arp::ArpPacket;
use crate::error;
use crate::executor::sleep;
use crate::executor::spawn_global;
use crate::executor::yield_execution;
use crate::font::get_glyph_width;
use crate::graphics::draw_button;
use crate::graphics::draw_str_fg;
use crate::graphics::fill_rect;
use crate::graphics::Rect;
use crate::gui::global_vram_resolutions;
use crate::gui::GLOBAL_VRAM;
use crate::hpet::global_timestamp;
use crate::icmp;
use crate::info;
use crate::init::EFI_MEMORY_MAP;
use crate::init::REBOOT_PARAMS;
use crate::input::MouseEvent;
use crate::input::PointerPosition;
use crate::input::GLOBAL_INPUT_MANAGER;
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
use core::time::Duration;

#[derive(Default)]
pub struct Console {
    prev_cmd: Option<String>,
    input_buf: String,
    ctrl_is_pressed: bool,
    is_ime_enabled: bool,
}
impl Console {
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
                    self.is_ime_enabled = !self.is_ime_enabled;
                    let (vw, vh) = global_vram_resolutions();
                    let buf = &mut *GLOBAL_VRAM.lock();
                    fill_rect(buf, 0x000000, vw - 16, vh - 16, 16, 16).unwrap();
                    draw_str_fg(
                        buf,
                        vw - 16,
                        vh - 16,
                        0xffffff,
                        if self.is_ime_enabled { "あ" } else { "Aa" },
                    );
                    return;
                }
                let c = if self.is_ime_enabled {
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
            "ping" => run_cmd_ping(&args),
            "reboot" | "r" => run_cmd_reboot(&args),
            "demo" => run_cmd_demo(&args),
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

    // Resolve dst MAC; if missing, prod the network with an ARP
    // request and wait briefly for a reply to land in the cache.
    let dst_mac = match nic::arp_lookup(target) {
        Some(m) => m,
        None => {
            nic::enqueue_tx_frame(
                ArpPacket::request(our_mac, nic::OUR_IP, target)
                    .as_slice()
                    .to_vec(),
            );
            sleep(PING_ARP_WAIT).await;
            nic::arp_lookup(target).ok_or("ARP unresolved")?
        }
    };

    let id: u16 = 0x1d10;
    let payload = [0xa5u8; PING_PAYLOAD_LEN];
    let frame = icmp::echo_request_frame(
        our_mac,
        nic::OUR_IP,
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
