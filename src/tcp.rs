extern crate alloc;

use crate::checksum::InternetChecksum;
use crate::checksum::InternetChecksumGenerator;
use crate::eth::EthernetAddr;
use crate::eth::EthernetHeader;
use crate::eth::EthernetType;
use crate::ip::IpV4Addr;
use crate::ip::IpV4Packet;
use crate::ip::IpV4Protocol;
use crate::mutex::Mutex;
use crate::slice::Sliceable;
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::mem::size_of;
use core::time::Duration;

// Fixed retransmission timeout. With no RTT estimation we just pick a
// value comfortably above the typical interactive round-trip on a USB
// link (single-digit milliseconds). Too low burns the link with
// duplicates; too high makes a lost segment feel like a hang.
const TCP_RTO: Duration = Duration::from_millis(500);

// How long the connection can sit idle (no rx and no tx) before we
// emit a keepalive probe. Picked low enough that the USB link on the
// host stays out of autosuspend (default ~2s on Linux).
const TCP_KEEPALIVE_INTERVAL: Duration = Duration::from_millis(1000);

// Conservative MSS for an Ethernet/IPv4 link without IP options:
// 1500 (MTU) - 20 (IP) - 20 (TCP) = 1460. We don't do IP fragmentation,
// so segments larger than this would be dropped at the NIC anyway.
const TCP_MSS: usize = 1460;

// TCP header per RFC 9293 §3.1, layered over IpV4Packet (which itself
// embeds the Ethernet header). Total prefix is 14 + 20 + 20 = 54 bytes
// before the TCP payload.
#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct TcpPacket {
    pub ip: IpV4Packet,
    src_port: [u8; 2],
    dst_port: [u8; 2],
    seq_num: [u8; 4],
    ack_num: [u8; 4],
    // flags[0] = Data Offset (top 4 bits) | Reserved (bottom 4 bits)
    // flags[1] = control flags (CWR ECE URG ACK PSH RST SYN FIN, MSB..LSB)
    flags: [u8; 2],
    window: [u8; 2],
    pub csum: InternetChecksum,
    urgent_ptr: [u8; 2],
}
const _: () = assert!(size_of::<TcpPacket>() - size_of::<IpV4Packet>() == 20);
unsafe impl Sliceable for TcpPacket {}

impl TcpPacket {
    pub fn src_port(&self) -> u16 {
        u16::from_be_bytes(self.src_port)
    }
    pub fn set_src_port(&mut self, port: u16) {
        self.src_port = port.to_be_bytes();
    }
    pub fn dst_port(&self) -> u16 {
        u16::from_be_bytes(self.dst_port)
    }
    pub fn set_dst_port(&mut self, port: u16) {
        self.dst_port = port.to_be_bytes();
    }
    pub fn seq_num(&self) -> u32 {
        u32::from_be_bytes(self.seq_num)
    }
    pub fn set_seq_num(&mut self, seq: u32) {
        self.seq_num = seq.to_be_bytes();
    }
    pub fn ack_num(&self) -> u32 {
        u32::from_be_bytes(self.ack_num)
    }
    pub fn set_ack_num(&mut self, ack: u32) {
        self.ack_num = ack.to_be_bytes();
    }
    pub fn window(&self) -> u16 {
        u16::from_be_bytes(self.window)
    }
    pub fn set_window(&mut self, w: u16) {
        self.window = w.to_be_bytes();
    }
    pub fn header_len_bytes(&self) -> usize {
        4 * (self.flags[0] >> 4) as usize
    }
    pub fn set_header_len_nibble(&mut self, nibble: u8) {
        self.flags[0] = (nibble << 4) | (self.flags[0] & 0x0f);
    }
    pub fn is_fin(&self) -> bool {
        (self.flags[1] & (1 << 0)) != 0
    }
    pub fn set_fin(&mut self) {
        self.flags[1] |= 1 << 0;
    }
    pub fn is_syn(&self) -> bool {
        (self.flags[1] & (1 << 1)) != 0
    }
    pub fn set_syn(&mut self) {
        self.flags[1] |= 1 << 1;
    }
    pub fn is_rst(&self) -> bool {
        (self.flags[1] & (1 << 2)) != 0
    }
    pub fn set_rst(&mut self) {
        self.flags[1] |= 1 << 2;
    }
    pub fn is_ack(&self) -> bool {
        (self.flags[1] & (1 << 4)) != 0
    }
    pub fn set_ack(&mut self) {
        self.flags[1] |= 1 << 4;
    }
}

/// Internet checksum over the TCP segment with the IPv4 pseudo-header
/// prepended, per RFC 9293 §3.1. `segment` is the bytes from the TCP
/// header (including its own zeroed `csum` field) through the end of
/// the TCP payload. The result, when written into the segment's `csum`
/// field, makes a self-consistent packet (re-summing yields 0).
pub fn tcp_segment_checksum(
    segment: &[u8],
    src: IpV4Addr,
    dst: IpV4Addr,
) -> InternetChecksum {
    let mut g = InternetChecksumGenerator::new();
    g.feed(segment);
    g.feed(&src.bytes());
    g.feed(&dst.bytes());
    // 0x00 || protocol(=6 for TCP) || length-in-be-bytes.
    g.feed(&[0x00, 0x06]);
    g.feed(&(segment.len() as u16).to_be_bytes());
    g.checksum()
}

// Subset of RFC 9293 §3.3.2 states actually traversed by a passive
// (server) socket that doesn't initiate active close. Initial open
// (SynSent) and active-close states (FinWait*, Closing, TimeWait) are
// intentionally omitted — the server reaches the four-way close from
// CloseWait/LastAck only.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TcpSocketState {
    Listen,
    SynReceived,
    Established,
    LastAck,
}

struct TcpSocketInner {
    state: TcpSocketState,
    peer_mac: EthernetAddr,
    peer_ip: IpV4Addr,
    peer_port: u16,
    my_next_seq: u32,
    last_seq_to_ack: u32,
    rx_data: VecDeque<u8>,
    // `send_buffer[0..unacked_len]` are bytes that have been transmitted
    // and are waiting for an ACK; `send_buffer[unacked_len..]` are queued
    // for first transmission. ACKs drain bytes from the front.
    send_buffer: VecDeque<u8>,
    unacked_len: usize,
    last_tx_at: Option<Duration>,
    // Updated on every rx and every tx; drives the keepalive timer.
    last_active_at: Option<Duration>,
}

pub struct TcpSocket {
    listen_port: u16,
    inner: Mutex<TcpSocketInner>,
}

/// Global TCP listener used by the NCM driver for the remote terminal.
/// One connection at a time on port 23.
pub static TCP_SOCKET: TcpSocket = TcpSocket::new_server(23);

impl TcpSocket {
    pub const fn new_server(listen_port: u16) -> Self {
        Self {
            listen_port,
            inner: Mutex::new(TcpSocketInner {
                state: TcpSocketState::Listen,
                peer_mac: EthernetAddr::zero(),
                peer_ip: IpV4Addr::new([0, 0, 0, 0]),
                peer_port: 0,
                // RFC 9293 doesn't require any specific ISS; a fixed value
                // is fine for our purposes since this isn't security-grade.
                my_next_seq: 1234,
                last_seq_to_ack: 0,
                rx_data: VecDeque::new(),
                send_buffer: VecDeque::new(),
                unacked_len: 0,
                last_tx_at: None,
                last_active_at: None,
            }),
        }
    }
    pub fn state(&self) -> TcpSocketState {
        self.inner.lock().state
    }
    /// Snapshot of socket state for diagnostics. Uses `try_lock` so
    /// this can be called from log / panic paths without risking the
    /// 10000-spin-then-panic cycle if the socket is already locked.
    pub fn debug_summary(&self) -> Option<(TcpSocketState, usize, usize)> {
        let inner = self.inner.try_lock().ok()?;
        Some((inner.state, inner.unacked_len, inner.send_buffer.len()))
    }
    pub fn pop_rx_byte(&self) -> Option<u8> {
        self.inner.lock().rx_data.pop_front()
    }
    /// Append bytes to the TX queue, but only while a connection is
    /// Established. Bytes produced before the first connect or after
    /// disconnect are dropped on the floor — we don't want to dump a
    /// boot-time backlog into a freshly-connected client.
    pub fn push_tx_bytes(&self, data: &[u8]) {
        let mut inner = self.inner.lock();
        if inner.state == TcpSocketState::Established {
            inner.send_buffer.extend(data.iter().copied());
        }
    }

    /// Drive the state machine for one received frame
    /// (Ethernet+IPv4+TCP+data). Returns the immediate reply frame, if
    /// any (SYN+ACK on SYN, ACK on data, FIN+ACK on FIN, etc.).
    /// `now` records arrival time for the keepalive timer.
    pub fn handle_rx(
        &self,
        frame: &[u8],
        our_mac: EthernetAddr,
        our_ip: IpV4Addr,
        now: Duration,
    ) -> Option<Vec<u8>> {
        if frame.len() < size_of::<TcpPacket>() {
            return None;
        }
        let in_tcp =
            TcpPacket::copy_from_slice(&frame[..size_of::<TcpPacket>()])
                .ok()?;
        if in_tcp.dst_port() != self.listen_port {
            return None;
        }

        let tcp_total = in_tcp.ip.total_length()
            - (size_of::<IpV4Packet>() - size_of::<EthernetHeader>());
        let header_len = in_tcp.header_len_bytes();
        if header_len < 20 || tcp_total < header_len {
            return None;
        }
        let data_start = size_of::<IpV4Packet>() + header_len;
        let data_end = size_of::<IpV4Packet>()
            + tcp_total.min(frame.len() - size_of::<IpV4Packet>());
        let data: &[u8] = if data_end > data_start {
            &frame[data_start..data_end]
        } else {
            &[]
        };

        let mut inner = self.inner.lock();
        inner.last_active_at = Some(now);

        // RST teardown: if the peer reset us at any point, drop straight
        // back to Listen so the next SYN can be accepted. Without this
        // the socket can wedge in Established forever after a peer
        // crash or middlebox reset.
        if in_tcp.is_rst() {
            inner.state = TcpSocketState::Listen;
            inner.peer_mac = EthernetAddr::zero();
            inner.peer_ip = IpV4Addr::new([0, 0, 0, 0]);
            inner.peer_port = 0;
            inner.last_seq_to_ack = 0;
            inner.rx_data.clear();
            inner.send_buffer.clear();
            inner.unacked_len = 0;
            inner.last_tx_at = None;
            return None;
        }

        let prev_state = inner.state;
        let mut seq_to_ack = in_tcp.seq_num();
        let seq = inner.my_next_seq;
        let mut send_syn = false;
        let mut send_fin = false;

        match prev_state {
            TcpSocketState::Listen => {
                if !in_tcp.is_syn() {
                    return None;
                }
                // SYN consumes one seq-space slot.
                seq_to_ack = seq_to_ack.wrapping_add(1);
                send_syn = true;
                inner.peer_mac = in_tcp.ip.eth.src();
                inner.peer_ip = in_tcp.ip.src();
                inner.peer_port = in_tcp.src_port();
                inner.my_next_seq = seq.wrapping_add(1);
                inner.state = TcpSocketState::SynReceived;
            }
            TcpSocketState::SynReceived => {
                if !in_tcp.is_ack() || in_tcp.ack_num() != inner.my_next_seq {
                    return None;
                }
                inner.state = TcpSocketState::Established;
                inner.last_seq_to_ack = in_tcp.seq_num();
                return None;
            }
            TcpSocketState::Established => {
                // Free any of our previously-sent bytes the peer has
                // now ACKed. The wrapping subtraction handles seq-space
                // wrap correctly: a stale or out-of-range ack_num
                // produces a value larger than `unacked_len` and is
                // ignored.
                if in_tcp.is_ack() && inner.unacked_len > 0 {
                    let first_unacked_seq = inner
                        .my_next_seq
                        .wrapping_sub(inner.unacked_len as u32);
                    let acked = in_tcp.ack_num().wrapping_sub(first_unacked_seq)
                        as usize;
                    if acked > 0 && acked <= inner.unacked_len {
                        inner.send_buffer.drain(..acked);
                        inner.unacked_len -= acked;
                        if inner.unacked_len == 0 {
                            inner.last_tx_at = None;
                        }
                    }
                }
                if !data.is_empty() {
                    inner.rx_data.extend(data.iter().copied());
                    seq_to_ack = seq_to_ack.wrapping_add(data.len() as u32);
                }
                if in_tcp.is_fin() {
                    seq_to_ack = seq_to_ack.wrapping_add(1);
                    send_fin = true;
                    inner.state = TcpSocketState::LastAck;
                    inner.my_next_seq = seq.wrapping_add(1);
                }
                if data.is_empty() && !in_tcp.is_fin() {
                    // Bare ACK from peer (e.g., ACKing our data) — nothing
                    // to send back.
                    return None;
                }
            }
            TcpSocketState::LastAck => {
                if in_tcp.is_ack() {
                    inner.state = TcpSocketState::Listen;
                    inner.peer_mac = EthernetAddr::zero();
                    inner.peer_ip = IpV4Addr::new([0, 0, 0, 0]);
                    inner.peer_port = 0;
                    inner.last_seq_to_ack = 0;
                    inner.rx_data.clear();
                    inner.send_buffer.clear();
                    inner.unacked_len = 0;
                    inner.last_tx_at = None;
                }
                return None;
            }
        }

        inner.last_seq_to_ack = seq_to_ack;
        let peer_mac = inner.peer_mac;
        let peer_ip = inner.peer_ip;
        let peer_port = inner.peer_port;
        drop(inner);

        Some(build_segment(
            our_mac,
            peer_mac,
            our_ip,
            peer_ip,
            self.listen_port,
            peer_port,
            seq,
            Some(seq_to_ack),
            send_syn,
            send_fin,
            &[],
        ))
    }

    /// Periodic transmit pump. Returns at most one segment per call:
    /// either a retransmit of the oldest unacked bytes (if RTO has
    /// elapsed since they were last sent), or a fresh segment carrying
    /// any newly queued bytes.
    ///
    /// `now` is the caller's monotonic clock — pulled from
    /// `hpet::global_timestamp` in production, an arbitrary `Duration`
    /// in unit tests.
    pub fn poll_tx(
        &self,
        our_mac: EthernetAddr,
        our_ip: IpV4Addr,
        now: Duration,
    ) -> Option<Vec<u8>> {
        let mut inner = self.inner.lock();
        if inner.state != TcpSocketState::Established {
            return None;
        }

        // Three things this tick might do, in priority order:
        //   1. retransmit unacked bytes whose RTO has elapsed
        //   2. ship newly queued bytes
        //   3. emit a keepalive probe if the connection has been idle
        let need_retransmit = inner.unacked_len > 0
            && inner
                .last_tx_at
                .map_or(true, |t| now.saturating_sub(t) >= TCP_RTO);
        let has_fresh = inner.send_buffer.len() > inner.unacked_len;

        let (data, seq) = if need_retransmit {
            // Cap retransmit at MSS so an unbounded unacked window
            // doesn't produce an oversized Ethernet frame the NIC
            // would drop. The rest stays unacked and will be picked
            // up on a later RTO tick.
            let take_n = inner.unacked_len.min(TCP_MSS);
            let bytes: Vec<u8> =
                inner.send_buffer.iter().take(take_n).copied().collect();
            let first_unacked_seq =
                inner.my_next_seq.wrapping_sub(inner.unacked_len as u32);
            inner.last_tx_at = Some(now);
            (bytes, first_unacked_seq)
        } else if has_fresh {
            // Same MSS cap on the fresh-data path. Larger pushes
            // drain across multiple poll_tx ticks.
            let bytes: Vec<u8> = inner
                .send_buffer
                .iter()
                .skip(inner.unacked_len)
                .take(TCP_MSS)
                .copied()
                .collect();
            let seq = inner.my_next_seq;
            inner.my_next_seq = seq.wrapping_add(bytes.len() as u32);
            inner.unacked_len += bytes.len();
            inner.last_tx_at = Some(now);
            (bytes, seq)
        } else {
            // Keepalive: nothing to send, but if the connection has
            // been idle for KEEPALIVE_INTERVAL, send a pure ACK with
            // seq = SND.NXT - 1. Peer treats it as a duplicate of an
            // already-received byte and responds with a current ACK,
            // which lands as activity and resets our timer.
            let last = inner.last_active_at.unwrap_or(Duration::ZERO);
            if now.saturating_sub(last) < TCP_KEEPALIVE_INTERVAL {
                return None;
            }
            (Vec::new(), inner.my_next_seq.wrapping_sub(1))
        };

        inner.last_active_at = Some(now);
        let seq_to_ack = inner.last_seq_to_ack;
        let peer_mac = inner.peer_mac;
        let peer_ip = inner.peer_ip;
        let peer_port = inner.peer_port;
        drop(inner);

        Some(build_segment(
            our_mac,
            peer_mac,
            our_ip,
            peer_ip,
            self.listen_port,
            peer_port,
            seq,
            Some(seq_to_ack),
            false,
            false,
            &data,
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn build_segment(
    our_mac: EthernetAddr,
    peer_mac: EthernetAddr,
    our_ip: IpV4Addr,
    peer_ip: IpV4Addr,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack_seq: Option<u32>,
    syn: bool,
    fin: bool,
    data: &[u8],
) -> Vec<u8> {
    let total_frame_len = size_of::<TcpPacket>() + data.len();
    let mut bytes = vec![0u8; total_frame_len];

    let eth = EthernetHeader::new(peer_mac, our_mac, EthernetType::ip_v4());
    let mut tcp = TcpPacket {
        ip: IpV4Packet::new(
            eth,
            peer_ip,
            our_ip,
            IpV4Protocol::tcp(),
            20 + data.len(),
        ),
        ..TcpPacket::default()
    };
    tcp.set_src_port(src_port);
    tcp.set_dst_port(dst_port);
    tcp.set_seq_num(seq);
    tcp.set_window(0xFFFF);
    tcp.set_header_len_nibble(5);
    if let Some(a) = ack_seq {
        tcp.set_ack();
        tcp.set_ack_num(a);
    }
    if syn {
        tcp.set_syn();
    }
    if fin {
        tcp.set_fin();
    }
    tcp.ip.recompute_checksum();

    bytes[..size_of::<TcpPacket>()].copy_from_slice(tcp.as_slice());
    bytes[size_of::<TcpPacket>()..].copy_from_slice(data);

    let segment_off = size_of::<IpV4Packet>();
    let csum = tcp_segment_checksum(&bytes[segment_off..], our_ip, peer_ip);
    let csum_off_in_tcp = 16; // offset of csum within the TCP header
    bytes[segment_off + csum_off_in_tcp..segment_off + csum_off_in_tcp + 2]
        .copy_from_slice(&csum.bytes());

    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test_case]
    fn tcp_packet_header_size_is_20() {
        assert_eq!(size_of::<TcpPacket>() - size_of::<IpV4Packet>(), 20);
    }

    #[test_case]
    fn tcp_flags_roundtrip() {
        let mut p = TcpPacket::default();
        assert!(!p.is_syn() && !p.is_ack() && !p.is_fin() && !p.is_rst());
        p.set_syn();
        assert!(p.is_syn() && !p.is_ack() && !p.is_fin() && !p.is_rst());
        p.set_ack();
        assert!(p.is_syn() && p.is_ack() && !p.is_fin() && !p.is_rst());
        let mut q = TcpPacket::default();
        q.set_fin();
        assert!(q.is_fin() && !q.is_syn() && !q.is_ack() && !q.is_rst());
        let mut r = TcpPacket::default();
        r.set_rst();
        assert!(r.is_rst() && !r.is_syn() && !r.is_ack() && !r.is_fin());
    }

    #[test_case]
    fn tcp_header_len_nibble_round_trip() {
        let mut p = TcpPacket::default();
        p.set_header_len_nibble(5);
        assert_eq!(p.header_len_bytes(), 20);
        // Setting Data Offset must not clobber the control flags.
        p.set_syn();
        p.set_header_len_nibble(8);
        assert_eq!(p.header_len_bytes(), 32);
        assert!(p.is_syn());
    }

    fn build_client_segment(
        peer_mac: EthernetAddr,
        peer_ip: IpV4Addr,
        peer_port: u16,
        our_mac: EthernetAddr,
        our_ip: IpV4Addr,
        listen_port: u16,
        seq: u32,
        ack_seq: Option<u32>,
        syn: bool,
        fin: bool,
        data: &[u8],
    ) -> Vec<u8> {
        // Reuse build_segment with peer/us swapped.
        super::build_segment(
            peer_mac,
            our_mac,
            peer_ip,
            our_ip,
            peer_port,
            listen_port,
            seq,
            ack_seq,
            syn,
            fin,
            data,
        )
    }

    fn parse_reply(reply: &[u8]) -> TcpPacket {
        TcpPacket::copy_from_slice(&reply[..size_of::<TcpPacket>()]).unwrap()
    }

    const PEER_MAC: EthernetAddr =
        EthernetAddr::new([0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA]);
    const PEER_IP: IpV4Addr = IpV4Addr::new([10, 10, 10, 1]);
    const OUR_MAC: EthernetAddr =
        EthernetAddr::new([0x11, 0x11, 0x11, 0x11, 0x11, 0x11]);
    const OUR_IP: IpV4Addr = IpV4Addr::new([10, 10, 10, 83]);

    #[test_case]
    fn handle_rx_listen_to_syn_received() {
        let sock = TcpSocket::new_server(23);
        let syn = build_client_segment(
            PEER_MAC,
            PEER_IP,
            12345,
            OUR_MAC,
            OUR_IP,
            23,
            5000,
            None,
            true,
            false,
            &[],
        );
        let reply = sock
            .handle_rx(&syn, OUR_MAC, OUR_IP, Duration::ZERO)
            .unwrap();
        let r = parse_reply(&reply);
        assert!(r.is_syn() && r.is_ack());
        assert_eq!(r.ack_num(), 5001); // SYN consumed one seq
        assert_eq!(r.src_port(), 23);
        assert_eq!(r.dst_port(), 12345);
        assert_eq!(sock.state(), TcpSocketState::SynReceived);
    }

    #[test_case]
    fn handle_rx_synreceived_to_established() {
        let sock = TcpSocket::new_server(23);
        let syn = build_client_segment(
            PEER_MAC,
            PEER_IP,
            12345,
            OUR_MAC,
            OUR_IP,
            23,
            5000,
            None,
            true,
            false,
            &[],
        );
        let synack = sock
            .handle_rx(&syn, OUR_MAC, OUR_IP, Duration::ZERO)
            .unwrap();
        let r = parse_reply(&synack);
        let our_seq_plus_1 = r.seq_num().wrapping_add(1);

        // Client's ACK of the SYN+ACK.
        let ack = build_client_segment(
            PEER_MAC,
            PEER_IP,
            12345,
            OUR_MAC,
            OUR_IP,
            23,
            5001,
            Some(our_seq_plus_1),
            false,
            false,
            &[],
        );
        let reply = sock.handle_rx(&ack, OUR_MAC, OUR_IP, Duration::ZERO);
        assert!(reply.is_none());
        assert_eq!(sock.state(), TcpSocketState::Established);
    }

    fn established_socket() -> (TcpSocket, u32) {
        let sock = TcpSocket::new_server(23);
        let syn = build_client_segment(
            PEER_MAC,
            PEER_IP,
            12345,
            OUR_MAC,
            OUR_IP,
            23,
            5000,
            None,
            true,
            false,
            &[],
        );
        let synack = sock
            .handle_rx(&syn, OUR_MAC, OUR_IP, Duration::ZERO)
            .unwrap();
        let our_seq_plus_1 = parse_reply(&synack).seq_num().wrapping_add(1);
        let ack = build_client_segment(
            PEER_MAC,
            PEER_IP,
            12345,
            OUR_MAC,
            OUR_IP,
            23,
            5001,
            Some(our_seq_plus_1),
            false,
            false,
            &[],
        );
        sock.handle_rx(&ack, OUR_MAC, OUR_IP, Duration::ZERO);
        (sock, 5001)
    }

    #[test_case]
    fn handle_rx_data_acks_and_buffers() {
        let (sock, client_seq) = established_socket();
        let data = b"hello";
        let seg = build_client_segment(
            PEER_MAC,
            PEER_IP,
            12345,
            OUR_MAC,
            OUR_IP,
            23,
            client_seq,
            Some(0),
            false,
            false,
            data,
        );
        let reply = sock
            .handle_rx(&seg, OUR_MAC, OUR_IP, Duration::ZERO)
            .unwrap();
        let r = parse_reply(&reply);
        assert!(r.is_ack());
        assert!(!r.is_syn() && !r.is_fin());
        assert_eq!(r.ack_num(), client_seq.wrapping_add(data.len() as u32));
        // rx_data was populated with the payload.
        let mut got = Vec::new();
        while let Some(b) = sock.pop_rx_byte() {
            got.push(b);
        }
        assert_eq!(got, data);
    }

    #[test_case]
    fn poll_tx_keeps_bytes_until_acked() {
        let (sock, _client_seq) = established_socket();
        sock.push_tx_bytes(b"hello");
        // First poll: ships the data, advances my_next_seq.
        let seg1 = sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(0))
            .unwrap();
        let p1 = parse_reply(&seg1);
        let first_seq = p1.seq_num();
        // Within RTO and no new data: nothing to do.
        assert!(sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(100))
            .is_none());
        // ACK arrives covering the 5 bytes.
        let ack = build_client_segment(
            PEER_MAC,
            PEER_IP,
            12345,
            OUR_MAC,
            OUR_IP,
            23,
            5001,
            Some(first_seq.wrapping_add(5)),
            false,
            false,
            &[],
        );
        sock.handle_rx(&ack, OUR_MAC, OUR_IP, Duration::from_millis(700));
        // Past RTO but still within the keepalive window, with nothing
        // unacked: no segment to send.
        assert!(sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(900))
            .is_none());
    }

    #[test_case]
    fn poll_tx_retransmits_after_rto() {
        let (sock, _) = established_socket();
        sock.push_tx_bytes(b"hello");
        let seg1 = sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(0))
            .unwrap();
        let p1 = parse_reply(&seg1);
        let original_seq = p1.seq_num();

        // Within RTO -> nothing.
        assert!(sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(400))
            .is_none());

        // After RTO with no ACK: retransmit. Same seq, same payload.
        let seg2 = sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(600))
            .unwrap();
        let p2 = parse_reply(&seg2);
        assert_eq!(p2.seq_num(), original_seq);
        assert_eq!(seg2.len(), seg1.len());
        assert_eq!(
            &seg2[size_of::<TcpPacket>()..],
            &seg1[size_of::<TcpPacket>()..]
        );
    }

    #[test_case]
    fn poll_tx_fresh_data_after_partial_ack() {
        let (sock, _) = established_socket();
        sock.push_tx_bytes(b"hello");
        let seg1 = sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(0))
            .unwrap();
        let first_seq = parse_reply(&seg1).seq_num();

        // ACK the 5 bytes, queue 3 more.
        let ack = build_client_segment(
            PEER_MAC,
            PEER_IP,
            12345,
            OUR_MAC,
            OUR_IP,
            23,
            5001,
            Some(first_seq.wrapping_add(5)),
            false,
            false,
            &[],
        );
        sock.handle_rx(&ack, OUR_MAC, OUR_IP, Duration::ZERO);
        sock.push_tx_bytes(b"abc");

        // Next poll sends the fresh 3 bytes with seq right after "hello".
        let seg2 = sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(100))
            .unwrap();
        let p2 = parse_reply(&seg2);
        assert_eq!(p2.seq_num(), first_seq.wrapping_add(5));
        assert_eq!(&seg2[size_of::<TcpPacket>()..], b"abc");
    }

    #[test_case]
    fn poll_tx_caps_segment_at_mss_for_fresh_data() {
        let (sock, _) = established_socket();
        // Push more than one MSS of data.
        let big = alloc::vec![b'A'; TCP_MSS * 3];
        sock.push_tx_bytes(&big);
        let seg = sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(0))
            .unwrap();
        // The full Ethernet+IP+TCP+payload frame should not exceed
        // the size_of::<TcpPacket>() + TCP_MSS bound.
        assert!(seg.len() <= size_of::<TcpPacket>() + TCP_MSS);
        assert_eq!(seg.len() - size_of::<TcpPacket>(), TCP_MSS);
    }

    #[test_case]
    fn poll_tx_caps_segment_at_mss_on_retransmit() {
        let (sock, _) = established_socket();
        // First send: ships TCP_MSS bytes, advances unacked_len.
        let big = alloc::vec![b'B'; TCP_MSS * 2];
        sock.push_tx_bytes(&big);
        let _ = sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(0))
            .unwrap();
        // Force a retransmit by jumping past RTO with no ACK.
        let seg = sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(600))
            .unwrap();
        assert_eq!(seg.len() - size_of::<TcpPacket>(), TCP_MSS);
    }

    #[test_case]
    fn send_buffer_drains_across_many_poll_tx_acks() {
        // A round-trip-y simulation: push 5 * MSS bytes, then for each
        // poll_tx send + ACK pair confirm send_buffer shrinks
        // monotonically and seq numbers march forward correctly.
        let (sock, _) = established_socket();
        let payload = alloc::vec![b'C'; TCP_MSS * 5];
        sock.push_tx_bytes(&payload);

        let mut now = Duration::from_millis(0);
        let mut total_sent: u32 = 0;
        for _ in 0..5 {
            let seg = sock.poll_tx(OUR_MAC, OUR_IP, now).unwrap();
            let p = parse_reply(&seg);
            let payload_len = (seg.len() - size_of::<TcpPacket>()) as u32;
            assert_eq!(payload_len, TCP_MSS as u32);
            // ACK the bytes we just sent.
            let ack = build_client_segment(
                PEER_MAC,
                PEER_IP,
                12345,
                OUR_MAC,
                OUR_IP,
                23,
                5001,
                Some(p.seq_num().wrapping_add(payload_len)),
                false,
                false,
                &[],
            );
            sock.handle_rx(&ack, OUR_MAC, OUR_IP, now);
            total_sent += payload_len;
            now = now + Duration::from_millis(10);
        }
        // After all five chunks ACKed, retransmit pressure should be
        // gone and there should be nothing left to send.
        assert_eq!(total_sent, (TCP_MSS * 5) as u32);
        assert!(sock
            .poll_tx(OUR_MAC, OUR_IP, now + Duration::from_millis(10))
            .is_none());
    }

    #[test_case]
    fn poll_tx_emits_keepalive_when_idle() {
        let (sock, _) = established_socket();
        // Right after connect, we're not yet "idle" — last_active_at
        // was set during the handshake (Duration::ZERO).
        // Inside KEEPALIVE_INTERVAL: nothing to send.
        assert!(sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(500))
            .is_none());
        // Past KEEPALIVE_INTERVAL: send a keepalive.
        let seg = sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(1500))
            .unwrap();
        let r = parse_reply(&seg);
        assert!(r.is_ack());
        assert!(!r.is_syn() && !r.is_fin());
        // No data payload — segment is just the TCP header.
        assert_eq!(seg.len(), size_of::<TcpPacket>());
        // seq is SND.NXT - 1 (the "old byte" probe trick).
        // After established_socket, my_next_seq is the original ISN+1
        // from the SYN, with no data sent yet. So keepalive seq is
        // my_next_seq - 1 = ISN.
        // The exact ISN is internal, so check the relationship:
        // a fresh push then poll_tx uses my_next_seq, which == seq + 1.
        sock.push_tx_bytes(b"x");
        let next = sock
            .poll_tx(OUR_MAC, OUR_IP, Duration::from_millis(1500))
            .unwrap();
        let r2 = parse_reply(&next);
        assert_eq!(r2.seq_num(), r.seq_num().wrapping_add(1));
    }

    #[test_case]
    fn handle_rx_rst_returns_to_listen() {
        let (sock, client_seq) = established_socket();
        // Build a RST segment from the peer.
        let rst = {
            let mut bytes = build_client_segment(
                PEER_MAC,
                PEER_IP,
                12345,
                OUR_MAC,
                OUR_IP,
                23,
                client_seq,
                Some(0),
                false,
                false,
                &[],
            );
            // Patch flags byte to set RST and clear ACK.
            let flags_off = size_of::<IpV4Packet>() + 13;
            bytes[flags_off] = 1 << 2; // RST only
            bytes
        };
        sock.handle_rx(&rst, OUR_MAC, OUR_IP, Duration::ZERO);
        assert_eq!(sock.state(), TcpSocketState::Listen);
    }

    #[test_case]
    fn handle_rx_fin_to_lastack_then_close() {
        let (sock, client_seq) = established_socket();
        let fin = build_client_segment(
            PEER_MAC,
            PEER_IP,
            12345,
            OUR_MAC,
            OUR_IP,
            23,
            client_seq,
            Some(0),
            false,
            true,
            &[],
        );
        let reply = sock
            .handle_rx(&fin, OUR_MAC, OUR_IP, Duration::ZERO)
            .unwrap();
        let r = parse_reply(&reply);
        assert!(r.is_fin() && r.is_ack());
        assert_eq!(r.ack_num(), client_seq.wrapping_add(1));
        assert_eq!(sock.state(), TcpSocketState::LastAck);

        // Client's final ACK closes us back to Listen.
        let our_seq = r.seq_num().wrapping_add(1);
        let ack = build_client_segment(
            PEER_MAC,
            PEER_IP,
            12345,
            OUR_MAC,
            OUR_IP,
            23,
            client_seq.wrapping_add(1),
            Some(our_seq),
            false,
            false,
            &[],
        );
        sock.handle_rx(&ack, OUR_MAC, OUR_IP, Duration::ZERO);
        assert_eq!(sock.state(), TcpSocketState::Listen);
    }

    #[test_case]
    fn tcp_segment_checksum_self_check() {
        // Build a SYN segment by hand with the csum field zeroed,
        // compute the checksum, then verify the standard self-check:
        // re-summing segment+pseudo-header with the csum field filled
        // in yields 0x0000.
        let src = IpV4Addr::new([10, 10, 10, 1]);
        let dst = IpV4Addr::new([10, 10, 10, 83]);
        let mut seg = vec![0u8; 20];
        // src_port=12345, dst_port=23
        seg[0..2].copy_from_slice(&12345u16.to_be_bytes());
        seg[2..4].copy_from_slice(&23u16.to_be_bytes());
        // seq=0xDEADBEEF, ack=0
        seg[4..8].copy_from_slice(&0xDEADBEEFu32.to_be_bytes());
        // Data Offset = 5 (=> 20 bytes), no flags except SYN.
        seg[12] = 5 << 4;
        seg[13] = 1 << 1; // SYN
                          // Window = 0xFFFF.
        seg[14..16].copy_from_slice(&0xFFFFu16.to_be_bytes());
        // csum (16..18) = 0, urgent (18..20) = 0.

        let csum = tcp_segment_checksum(&seg, src, dst);
        seg[16..18].copy_from_slice(&csum.bytes());

        let mut g = InternetChecksumGenerator::new();
        g.feed(&seg);
        g.feed(&src.bytes());
        g.feed(&dst.bytes());
        g.feed(&[0x00, 0x06]);
        g.feed(&(seg.len() as u16).to_be_bytes());
        assert_eq!(g.checksum(), InternetChecksum::default());
    }
}
