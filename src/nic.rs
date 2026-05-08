extern crate alloc;

use crate::arp::ArpPacket;
use crate::cui::Console;
use crate::eth::EthernetAddr;
use crate::executor::sleep;
use crate::executor::spawn_global;
use crate::executor::with_timeout;
use crate::executor::yield_execution;
use crate::icmp;
use crate::icmp::IcmpPacket;
use crate::info;
use crate::ip::IpV4Addr;
use crate::ip::IpV4Packet;
use crate::ip::IpV4Protocol;
use crate::keyboard::KeyEvent;
use crate::mutex::Mutex;
use crate::ncm;
use crate::print::hexdump_bytes;
use crate::result::Result;
use crate::slice::Sliceable;
use crate::tcp::TCP_SOCKET;
use crate::usb;
use crate::usb::descriptors_under_config;
use crate::usb::descriptors_under_interface;
use crate::usb::pick_interface_with_triple;
use crate::usb::EndpointDescriptor;
use crate::usb::UsbDescriptor;
use crate::usb::UsbDeviceDescriptor;
use crate::usb::UsbDeviceDriver;
use crate::warn;
use crate::xhci::Controller;
use crate::xhci::EventFuture;
use crate::xhci::NormalTrb;
use crate::xhci::TransferRing;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::time::Duration;

pub const OUR_IP: IpV4Addr = IpV4Addr::new([10, 10, 10, 83]);

// Our own MAC, learned from the device's iMacAddress descriptor during
// NCM init. Set once `run()` has finished negotiating, before the first
// frame is sent. Consumers (e.g. the `ping` command) read this to build
// outbound frames without having to thread the MAC through.
pub static OUR_MAC: Mutex<Option<EthernetAddr>> = Mutex::new(None);

// IP→MAC cache populated passively from observed traffic on bulk-IN
// (ARP packets — both requests and replies — and IPv4 frames). Lets
// outbound traffic skip an ARP round-trip when the peer has already
// announced itself, which on a cdc-ncm host is the common case.
pub static ARP_CACHE: Mutex<BTreeMap<IpV4Addr, EthernetAddr>> =
    Mutex::new(BTreeMap::new());

// In-flight `ping` request awaiting an echo reply. The rx path notes
// the round-trip into `reply_rtt` when a matching echo reply arrives.
// We only track one outstanding ping at a time — the cui dispatch is
// serialized and the command itself awaits before issuing the next.
pub struct PingPending {
    pub id: u16,
    pub seq: u16,
    pub sent_at: Duration,
    pub reply_rtt: Option<Duration>,
    pub reply_src: Option<IpV4Addr>,
}
pub static PING_PENDING: Mutex<Option<PingPending>> = Mutex::new(None);

pub fn our_mac() -> Option<EthernetAddr> {
    *OUR_MAC.lock()
}

pub fn arp_lookup(ip: IpV4Addr) -> Option<EthernetAddr> {
    ARP_CACHE.lock().get(&ip).copied()
}

pub fn learn_arp(ip: IpV4Addr, mac: EthernetAddr) {
    if mac == EthernetAddr::zero() || mac == EthernetAddr::broadcast() {
        return;
    }
    ARP_CACHE.lock().insert(ip, mac);
}

// Frames produced asynchronously by other tasks that `poll_bulk`
// drains and ships out the bulk-OUT endpoint at the bottom of each
// rx iteration.
static NET_TX_QUEUE: Mutex<VecDeque<Vec<u8>>> = Mutex::new(VecDeque::new());

pub fn enqueue_tx_frame(frame: Vec<u8>) {
    NET_TX_QUEUE.lock().push_back(frame);
}

// Parse a 12-character upper-case hex MAC string from the device's
// iMacAddress descriptor (CDC ECM 1.2 §5.4) into the 6 raw bytes.
fn parse_mac_hex_str(s: &str) -> Result<EthernetAddr> {
    if s.len() != 12 {
        return Err("MAC: expected 12-char hex string");
    }
    let mut mac = [0u8; 6];
    for (i, byte) in mac.iter_mut().enumerate() {
        let chunk = s.get(i * 2..i * 2 + 2).ok_or("MAC: short")?;
        *byte =
            u8::from_str_radix(chunk, 16).map_err(|_| "MAC: bad hex digit")?;
    }
    Ok(EthernetAddr::new(mac))
}

pub struct UsbNcmDriver;
impl UsbNcmDriver {
    pub async fn request_get_net_address(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
    ) -> Result<Vec<u8>> {
        let buf = vec![0u8; 6];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        xhc.request_transfer_from_class_interface(
            slot,
            ctrl_ep_ring,
            0x81,
            0,
            0,
            &mut buf,
        )
        .await?;
        Ok(buf.to_vec())
    }
    pub async fn request_get_ntb_parameters(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
    ) -> Result<Vec<u8>> {
        let buf = vec![0u8; 28];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        xhc.request_transfer_from_class_interface(
            slot,
            ctrl_ep_ring,
            0x80,
            0,
            0,
            &mut buf,
        )
        .await?;
        Ok(buf.to_vec())
    }
    pub async fn request_get_network_connection(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
    ) -> Result<Vec<u8>> {
        let buf = vec![0u8; 6];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        xhc.request_transfer_from_class_interface(
            slot,
            ctrl_ep_ring,
            0x81,
            0,
            0,
            &mut buf,
        )
        .await?;
        Ok(buf.to_vec())
    }
    async fn poll_int_in(
        xhc: Rc<Controller>,
        slot: u8,
        mut ring: TransferRing,
        desc: EndpointDescriptor,
    ) -> Result<()> {
        loop {
            let buf = vec![0u8; 16];
            let mut buf = Box::into_pin(buf.into_boxed_slice());
            let trb_ptr_waiting =
                ring.push(NormalTrb::new_in(&mut buf).into())?;
            let waiter = EventFuture::new_for_trb(
                &xhc.primary_event_ring,
                trb_ptr_waiting,
            );
            xhc.notify_ep(slot, desc.dci())?;

            if let Err(e) = waiter.await.map(|e| e.transfer_result_ok()) {
                info!("failed: {e:?}");
            } else {
                match buf[1] {
                    0x00 => {
                        info!(
                            "Notification: NETWORK_CONNECTION: {}",
                            if buf[2] == 1 {
                                "Connected"
                            } else {
                                "Disconnected"
                            }
                        );
                    }
                    0x2A => {
                        let downlink_bitrate = {
                            let mut v = [0u8; 4];
                            v.copy_from_slice(&buf[8..12]);
                            u32::from_le_bytes(v)
                        };
                        let uplink_bitrate = {
                            let mut v = [0u8; 4];
                            v.copy_from_slice(&buf[12..16]);
                            u32::from_le_bytes(v)
                        };
                        info!(
                            "Notification: CONNECTION_SPEED_CHANGE: \
                            up = {uplink_bitrate} bps, \
                            down = {downlink_bitrate} bps",
                        );
                    }
                    _ => {
                        info!("Notification: ?");
                        hexdump_bytes(&buf);
                    }
                }
            }
        }
    }
    async fn poll_bulk(
        xhc: Rc<Controller>,
        slot: u8,
        mut bulk_in_ring: TransferRing,
        bulk_in_desc: EndpointDescriptor,
        mut bulk_out_ring: TransferRing,
        bulk_out_desc: EndpointDescriptor,
        our_mac: EthernetAddr,
    ) -> Result<()> {
        // Announce ourselves so the host learns our MAC <-> IP binding.
        let arp = ArpPacket::gratuitous(our_mac, OUR_IP);
        Self::send_datagram(
            &xhc,
            slot,
            &mut bulk_out_ring,
            &bulk_out_desc,
            arp.as_slice(),
            0,
        )
        .await?;
        info!("NCM: sent gratuitous ARP for {OUR_IP}");

        info!("NCM: poll_bulk loop starting");
        let mut tx_seq: u16 = 1;
        let mut timeout_count: u32 = 0;
        // Cumulative byte counters since task start. Logged on every
        // bulk-in timeout so we can spot "stalls every N MB" patterns
        // tied to a specific buffer size.
        let mut bytes_rx: u64 = 0;
        let mut bytes_tx: u64 = 0;
        loop {
            // Big enough to hold an NTB wrapping a full Ethernet
            // frame (1514 + ~28 bytes of NCM headers). The previous
            // 1024-byte buffer silently dropped any NTB longer than
            // that, which forced the peer's TCP stack into its own
            // retransmit/backoff loop on every large segment.
            let buf = vec![0u8; 4096];
            let mut buf = Box::into_pin(buf.into_boxed_slice());
            let trb_ptr_waiting =
                bulk_in_ring.push(NormalTrb::new_in(&mut buf).into())?;

            xhc.notify_ep(slot, bulk_in_desc.dci())?;

            // Bound the wait *only* for periodic diagnostic logging.
            // We do NOT recycle the buffer or push a duplicate TRB
            // here: the ring has 15 usable slots, and an orphan TRB
            // permanently occupies its slot until the controller
            // generates a completion event for it. Once the producer
            // wraps to that slot the cycle-state check refuses to
            // overwrite, `push` returns "Command Ring is Full", and
            // the whole task dies — exactly the symptom we used to
            // ship. Instead we keep waiting on the same TRB; the
            // most common cause of a long wait is just "the device
            // hasn't started delivering NTBs yet" (idle period or
            // pre-link-up), which resolves itself once traffic
            // arrives.
            let event = loop {
                let fut = EventFuture::new_for_trb(
                    &xhc.primary_event_ring,
                    trb_ptr_waiting,
                );
                match with_timeout(Duration::from_secs(2), fut).await {
                    Ok(ev) => break ev,
                    Err(_) => {
                        timeout_count = timeout_count.wrapping_add(1);
                        let dbg = TCP_SOCKET.debug_summary();
                        let queued = NET_TX_QUEUE.lock().len();
                        warn!(
                            "NCM bulk-in: TRB {trb_ptr_waiting:#x} \
                             still waiting (#{timeout_count}); \
                             bytes_rx={bytes_rx}, \
                             bytes_tx={bytes_tx}, tcp={dbg:?}, \
                             net_tx_queue={queued}"
                        );
                    }
                }
            };
            if let Err(e) = event.transfer_result_ok() {
                warn!(
                    "NCM bulk-in: transfer error {e:?} on TRB \
                     {trb_ptr_waiting:#x}; dropping NTB"
                );
                continue;
            }
            let nth = match ncm::parse_nth16(&buf) {
                Ok(nth) => nth,
                Err(_) => continue,
            };
            let ntb_len = nth.block_length as usize;
            if ntb_len > buf.len() {
                warn!(
                    "NTB(seq={}): block_length {ntb_len} > buf {}",
                    nth.sequence,
                    buf.len()
                );
                continue;
            }
            bytes_rx = bytes_rx.saturating_add(ntb_len as u64);

            // Collect responses first so we don't borrow `buf` across an
            // await (`send_datagram` is async).
            let mut replies: Vec<Vec<u8>> = Vec::new();
            for frame in ncm::iter_ntb16_datagrams(&buf[..ntb_len]) {
                if frame.len() < 14 {
                    continue;
                }
                let eth_type = [frame[12], frame[13]];
                if eth_type == [0x08, 0x06] && frame.len() >= 42 {
                    if let Ok(req) = ArpPacket::copy_from_slice(&frame[..42]) {
                        // Whether request or reply, the sender's
                        // (ip, mac) pairing is authoritative for the
                        // cache.
                        learn_arp(req.sender_ip(), req.sender_mac());
                        if req.is_request_for(OUR_IP) {
                            info!(
                                "ARP: request for {} from {} ({:?})",
                                OUR_IP,
                                req.sender_ip(),
                                req.sender_mac(),
                            );
                            replies.push(
                                req.reply_to(our_mac).as_slice().to_vec(),
                            );
                        }
                    }
                } else if eth_type == [0x08, 0x00]
                    && frame.len() >= core::mem::size_of::<IpV4Packet>()
                {
                    if let Ok(ip) = IpV4Packet::copy_from_slice(
                        &frame[..core::mem::size_of::<IpV4Packet>()],
                    ) {
                        // Learn (src_ip -> src_mac) from any inbound
                        // IPv4 frame so outbound can resolve quickly.
                        learn_arp(ip.src(), ip.eth.src());
                        if ip.dst() != OUR_IP {
                            continue;
                        }
                        // Trim to ip.total_length() to drop any
                        // Ethernet-layer padding (frames < 60 bytes).
                        let frame_total = core::mem::size_of::<
                            crate::eth::EthernetHeader,
                        >() + ip.total_length();
                        let frame_total = frame_total.min(frame.len());
                        let frame = &frame[..frame_total];
                        if ip.protocol() == IpV4Protocol::icmp()
                            && frame.len() >= core::mem::size_of::<IcmpPacket>()
                        {
                            let icmp = IcmpPacket::copy_from_slice(
                                &frame[..core::mem::size_of::<IcmpPacket>()],
                            )
                            .ok();
                            if icmp
                                .map(|p| p.is_echo_request())
                                .unwrap_or(false)
                            {
                                match icmp::echo_reply_from_request(
                                    frame, our_mac, OUR_IP,
                                ) {
                                    Ok(reply) => {
                                        info!(
                                            "ICMP: echo req from {} -> reply",
                                            ip.src(),
                                        );
                                        replies.push(reply);
                                    }
                                    Err(e) => warn!("ICMP reply build: {e}"),
                                }
                            } else if let Some(p) = icmp {
                                if p.is_echo_reply() {
                                    let mut slot = PING_PENDING.lock();
                                    if let Some(pending) = slot.as_mut() {
                                        if pending.id == p.identifier()
                                            && pending.seq == p.sequence()
                                            && pending.reply_rtt.is_none()
                                        {
                                            let now =
                                                crate::hpet::global_timestamp();
                                            pending.reply_rtt =
                                                Some(now.saturating_sub(
                                                    pending.sent_at,
                                                ));
                                            pending.reply_src = Some(ip.src());
                                        }
                                    }
                                }
                            }
                        } else if ip.protocol() == IpV4Protocol::tcp() {
                            let now = crate::hpet::global_timestamp();
                            if let Some(reply) = TCP_SOCKET
                                .handle_rx(frame, our_mac, OUR_IP, now)
                            {
                                replies.push(reply);
                            }
                        }
                    }
                }
            }
            // Frames produced asynchronously by other tasks (e.g. the
            // periodic TCP TX poller) ride out on the same iteration.
            replies.extend(NET_TX_QUEUE.lock().drain(..));
            for reply in replies {
                let reply_len = reply.len() as u64;
                // Don't propagate send errors out of the loop —
                // dropping the task on a single bad TX would lose
                // the whole NCM pipeline. Log and keep going.
                if let Err(e) = Self::send_datagram(
                    &xhc,
                    slot,
                    &mut bulk_out_ring,
                    &bulk_out_desc,
                    &reply,
                    tx_seq,
                )
                .await
                {
                    warn!("NCM send_datagram: {e:?}; reply dropped");
                } else {
                    bytes_tx = bytes_tx.saturating_add(reply_len);
                }
                tx_seq = tx_seq.wrapping_add(1);
            }
        }
    }
    async fn poll_tcp_tx(our_mac: EthernetAddr) -> Result<()> {
        loop {
            let now = crate::hpet::global_timestamp();
            if let Some(frame) = TCP_SOCKET.poll_tx(our_mac, OUR_IP, now) {
                enqueue_tx_frame(frame);
            }
            sleep(Duration::from_millis(20)).await;
        }
    }
    /// Drive a Console from bytes the TCP peer types. The Console's
    /// `print!` calls are picked up by the TcpMirror tee in print.rs,
    /// so output reaches the remote without any explicit forwarding
    /// here.
    async fn drive_remote_console() -> Result<()> {
        // Three-state ANSI escape decoder for the cursor keys: a
        // terminal emits `ESC [ A` for Up, `ESC [ B` for Down, etc.
        // Without this the literal 'A' would slip through as input
        // and the leading ESC + '[' would be silently dropped.
        enum Ansi {
            Normal,
            SeenEsc,
            SeenCsi,
        }
        fn plain_byte_event(b: u8) -> Option<KeyEvent> {
            match b {
                b'\r' | b'\n' => Some(KeyEvent::Enter),
                // Most terminals (cooked-mode tty, telnet, most ssh
                // clients) send DEL=0x7F for the Backspace key. Map
                // both to our "erase prev char" event.
                0x7F | 0x08 => Some(KeyEvent::Char('\x08')),
                b if b.is_ascii_graphic() || b == b' ' => {
                    Some(KeyEvent::Char(b as char))
                }
                _ => None,
            }
        }

        let mut console = Console::default();
        let mut ansi = Ansi::Normal;
        loop {
            let b = match TCP_SOCKET.pop_rx_byte() {
                Some(b) => b,
                None => {
                    yield_execution().await;
                    continue;
                }
            };
            ansi = match ansi {
                Ansi::Normal => {
                    if b == 0x1B {
                        Ansi::SeenEsc
                    } else {
                        if let Some(e) = plain_byte_event(b) {
                            console.handle_key_down(e);
                        }
                        Ansi::Normal
                    }
                }
                Ansi::SeenEsc => {
                    if b == b'[' {
                        Ansi::SeenCsi
                    } else {
                        // Stray ESC — fall back to handling this
                        // byte as if it had arrived in Normal state
                        // so we don't eat the user's next keystroke.
                        if let Some(e) = plain_byte_event(b) {
                            console.handle_key_down(e);
                        }
                        Ansi::Normal
                    }
                }
                Ansi::SeenCsi => {
                    let ev = match b {
                        b'A' => Some(KeyEvent::CursorUp),
                        b'B' => Some(KeyEvent::CursorDown),
                        b'C' => Some(KeyEvent::CursorRight),
                        b'D' => Some(KeyEvent::CursorLeft),
                        _ => None,
                    };
                    if let Some(e) = ev {
                        console.handle_key_down(e);
                    }
                    Ansi::Normal
                }
            };
        }
    }
    async fn send_datagram(
        xhc: &Rc<Controller>,
        slot: u8,
        ring: &mut TransferRing,
        desc: &EndpointDescriptor,
        datagram: &[u8],
        seq: u16,
    ) -> Result<()> {
        let ntb = ncm::build_ntb16(datagram, seq);
        let buf = Box::into_pin(ntb.into_boxed_slice());
        let trb_ptr_waiting = ring.push(NormalTrb::new_out(&buf).into())?;
        xhc.notify_ep(slot, desc.dci())?;
        // Same orphan-avoidance dance as the bulk-in path: keep
        // waiting on the original TRB instead of pushing a duplicate.
        // See the comment in `poll_bulk` for the full reasoning.
        let mut waited: u32 = 0;
        let event = loop {
            let fut = EventFuture::new_for_trb(
                &xhc.primary_event_ring,
                trb_ptr_waiting,
            );
            match with_timeout(Duration::from_secs(2), fut).await {
                Ok(ev) => break ev,
                Err(_) => {
                    waited = waited.wrapping_add(1);
                    warn!(
                        "send_datagram: TRB {trb_ptr_waiting:#x} \
                         still waiting (#{waited}, len={})",
                        buf.len()
                    );
                }
            }
        };
        event.transfer_result_ok()?;
        Ok(())
    }
    async fn run(
        xhc: &Rc<Controller>,
        port: usize,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
        descriptors: &[UsbDescriptor],
    ) -> Result<()> {
        /*
        interface 0 alt 0 02:0D:00
                02: Communication Interface Class
                0D: Network Control Model Subclass
                00: Protocol defined in the USB Spec
            EP 1 interrupt in mps 16 interval 11
                SSEC 0, 0, 8, 0
        interface 1 alt 0 0A:00:01
        interface 1 alt 1 0A:00:01
            EP 2 bulk in mps 0x400 interval 0
                SSEC 5, 0, 0, 0
            EP 3 bulk out mps 0x400 interval 0
                SSEC 5, 0, 0, 0
        */
        let (config_desc, _, _) =
            pick_interface_with_triple(descriptors, (2, 13, 0))
                .ok_or("No USB NCM Communications interface found")?;
        info!("C: {config_desc:?}");
        let desc_under_config =
            descriptors_under_config(descriptors, config_desc.config_value());
        let mut mac_addr_index = 0;
        for d in &desc_under_config {
            if let UsbDescriptor::Interface(e) = d {
                info!("I:   {e:?}")
            } else if let UsbDescriptor::Endpoint(e) = d {
                info!("E:     {e:?}")
            } else if let UsbDescriptor::Unknown {
                desc_type: 0x30,
                payload,
                ..
            } = d
            {
                info!("SSEC:    {payload:?}")
            } else if let UsbDescriptor::Unknown {
                desc_type: 0x24, /* CS_INTERFACE [ncm_1_1] Table 6-2 */
                payload,
                ..
            } = d
            {
                let subtype = payload.first().cloned().unwrap_or_default();
                match subtype {
                    0x0F => {
                        /* Ethernet Networking Functional Descriptor [cdc_1_2
                         * Table 13] */
                        // Expected to be non-zero.
                        mac_addr_index =
                            payload.get(1).cloned().unwrap_or_default();
                    }
                    _ => {
                        info!("?   :    {d:?}")
                    }
                }
            } else if let UsbDescriptor::Unknown { .. } = d {
                info!("?   :    {d:?}")
            }
        }

        let mac_addr = {
            let res = with_timeout(
                Duration::from_secs(1),
                usb::request_string_descriptor_zero(xhc, slot, ctrl_ep_ring),
            )
            .await?;
            // If there is one lang_id, bLength will be 4
            if res[0] < 4 {
                return Err("string desc zero too short");
            }
            let lang_id = u16::from_le_bytes([res[2], res[3]]);
            with_timeout(
                Duration::from_secs(1),
                usb::request_string_descriptor(
                    xhc,
                    slot,
                    ctrl_ep_ring,
                    lang_id,
                    mac_addr_index,
                ),
            )
            .await?
        };
        info!("iMacAddress: {mac_addr:?}");

        //
        // Set up communications interface
        //

        let int_in_ep_desc = {
            let desc_under_com_interface =
                descriptors_under_interface(&desc_under_config, 0, 0);
            desc_under_com_interface
                .iter()
                .find_map(|d| {
                    if let usb::UsbDescriptor::Endpoint(d) = d {
                        if d.is_dir_in() && d.is_interrupt_endpoint() {
                            return Some(d);
                        }
                    }
                    None
                })
                .cloned()
                .ok_or("interrupt_in_ep_desc not found")
        }?;

        let bulk_in_ep_desc = {
            let desc_under_com_interface =
                descriptors_under_interface(&desc_under_config, 1, 1);

            desc_under_com_interface
                .iter()
                .find_map(|d| {
                    if let usb::UsbDescriptor::Endpoint(d) = d {
                        if d.is_dir_in() && d.is_bulk_endpoint() {
                            return Some(d);
                        }
                    }
                    None
                })
                .cloned()
                .ok_or("bulk_in_ep_desc not found")
        }?;

        let bulk_out_ep_desc = {
            let desc_under_com_interface =
                descriptors_under_interface(&desc_under_config, 1, 1);

            desc_under_com_interface
                .iter()
                .find_map(|d| {
                    if let usb::UsbDescriptor::Endpoint(d) = d {
                        if !d.is_dir_in() && d.is_bulk_endpoint() {
                            return Some(d);
                        }
                    }
                    None
                })
                .cloned()
                .ok_or("bulk_out_ep_desc not found")
        }?;

        let mut ring_list = usb::configure_endpoint(
            xhc,
            port,
            slot,
            &[int_in_ep_desc, bulk_in_ep_desc, bulk_out_ep_desc],
        )
        .await?;
        let int_in_ep_ring = ring_list
            .remove(&int_in_ep_desc.dci())
            .ok_or("ep_ring for interrupt in was not populated")?;
        let bulk_in_ep_ring = ring_list
            .remove(&bulk_in_ep_desc.dci())
            .ok_or("ep_ring for bulk in was not populated")?;
        let bulk_out_ep_ring = ring_list
            .remove(&bulk_out_ep_desc.dci())
            .ok_or("ep_ring for bulk out was not populated")?;

        xhc.request_set_config(slot, ctrl_ep_ring, 2).await?;
        xhc.request_set_interface(slot, ctrl_ep_ring, 0, 0).await?;
        // start operation!
        xhc.request_set_interface(slot, ctrl_ep_ring, 1, 1).await?;

        let ntbparams =
            Self::request_get_ntb_parameters(xhc, slot, ctrl_ep_ring).await?;
        info!("ntbparams: {ntbparams:?}");

        let our_mac = parse_mac_hex_str(&mac_addr)?;
        *OUR_MAC.lock() = Some(our_mac);
        info!("NCM: our MAC = {our_mac:?}, IP = {OUR_IP}");

        spawn_global(Self::poll_int_in(
            xhc.clone(),
            slot,
            int_in_ep_ring,
            int_in_ep_desc,
        ));
        spawn_global(Self::poll_bulk(
            xhc.clone(),
            slot,
            bulk_in_ep_ring,
            bulk_in_ep_desc,
            bulk_out_ep_ring,
            bulk_out_ep_desc,
            our_mac,
        ));
        spawn_global(Self::poll_tcp_tx(our_mac));
        spawn_global(Self::drive_remote_console());
        Ok(())
    }
}
impl UsbDeviceDriver for UsbNcmDriver {
    fn is_compatible(
        &self,
        descriptors: &[UsbDescriptor],
        _device_descriptor: &UsbDeviceDescriptor,
    ) -> bool {
        // Note: QEMU's usb-nic does not have this interface.
        pick_interface_with_triple(
            descriptors,
            (
                2,  /* Communications Interface Class [cdc_1_2] 4.2 */
                13, /* Network Control Model [cdc_1_2] 4.3 */
                0,
            ),
        )
        .is_some()
    }
    fn start(
        &self,
        xhc: Rc<Controller>,
        port: usize,
        slot: u8,
        mut ctrl_ep_ring: TransferRing,
        descriptors: Vec<UsbDescriptor>,
        _device_descriptor: &UsbDeviceDescriptor,
    ) {
        spawn_global(async move {
            Self::run(&xhc, port, slot, &mut ctrl_ep_ring, &descriptors).await
        });
    }
}
