extern crate alloc;

use crate::error::Result;
use crate::info;
use crate::mutex::Mutex;
use crate::net::checksum::InternetChecksum;
use crate::net::checksum::InternetChecksumGenerator;
use crate::net::eth::EthernetAddr;
use crate::net::eth::EthernetHeader;
use crate::net::eth::EthernetType;
use crate::net::ip::IpV4Packet;
use crate::net::ip::IpV4Protocol;
use crate::net::manager::Network;
use crate::warn;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::vec;
use alloc::vec::Vec;
use core::mem::size_of;
use noli::mem::Sliceable;
use noli::net::IpV4Addr;

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct TcpPacket {
    pub ip: IpV4Packet,
    src_port: [u8; 2],
    dst_port: [u8; 2],
    seq_num: [u8; 4],
    ack_num: [u8; 4],
    flags: [u8; 2],
    window: [u8; 2],
    pub csum: InternetChecksum,
    urgent_ptr: [u8; 2],
    // 20 bytes so far
    // Options follow...
    // [type: u8], [len: u8], [data: [u8; len]], ...
}
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
    pub fn set_seq_num(&mut self, seq_num: u32) {
        self.seq_num = seq_num.to_be_bytes();
    }
    pub fn ack_num(&self) -> u32 {
        u32::from_be_bytes(self.ack_num)
    }
    pub fn set_ack_num(&mut self, ack_num: u32) {
        self.ack_num = ack_num.to_be_bytes();
    }
    pub fn header_len(&self) -> usize {
        4 * (self.flags[0] >> 4) as usize
    }
    pub fn set_header_len_nibble(&mut self, header_len_nibble: u8) {
        // header_len_in_bytes = header_len_nibble * 4
        self.flags[0] = (header_len_nibble << 4) | (self.flags[0] & 0x0f);
    }
    pub fn flags(&self) -> u16 {
        u16::from_be_bytes(self.flags) & 0x0777
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
    pub fn is_ack(&self) -> bool {
        (self.flags[1] & (1 << 4)) != 0
    }
    pub fn set_ack(&mut self) {
        self.flags[1] |= 1 << 4;
    }
    pub fn window(&self) -> u16 {
        u16::from_be_bytes(self.window)
    }
    pub fn set_window(&mut self, window: u16) {
        self.window = window.to_be_bytes();
    }
}
unsafe impl Sliceable for TcpPacket {}
impl Debug for TcpPacket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "TCP :{} -> :{}, seq = {}, ack = {}, flags = {:#018b}{}{}{}",
            self.src_port(),
            self.dst_port(),
            self.seq_num(),
            self.ack_num(),
            self.flags(),
            if self.is_fin() { " FIN" } else { "" },
            if self.is_syn() { " SYN" } else { "" },
            if self.is_ack() { " ACK" } else { "" },
        )
    }
}

// https://datatracker.ietf.org/doc/html/rfc9293#name-state-machine-overview
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TcpSocketState {
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
    Closed,
    //
    PendingSynToBeSent,
}

// Step 1: Just receive (no error handling)
// Step 2: Just echo (no error handling)

pub struct TcpSocket {
    self_ip: Mutex<Option<IpV4Addr>>,
    self_port: Mutex<Option<u16>>,
    another_ip: Mutex<Option<IpV4Addr>>,
    another_port: Mutex<Option<u16>>,
    my_next_seq: Mutex<u32>,
    state: Mutex<TcpSocketState>,
}
impl Debug for TcpSocket {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "TcpSocket{{ state: {:?} }}", *self.state.lock())
    }
}
impl TcpSocket {
    pub fn new_server(src_port: u16) -> Self {
        Self {
            self_ip: Default::default(),
            self_port: Mutex::new(Some(src_port)),
            another_ip: Default::default(),
            another_port: Default::default(),
            my_next_seq: Mutex::new(0),
            state: Mutex::new(TcpSocketState::Listen),
        }
    }
    pub fn new_client(dst_ip: IpV4Addr, dst_port: u16) -> Self {
        Self {
            self_ip: Default::default(),
            self_port: Default::default(),
            another_ip: Mutex::new(Some(dst_ip)),
            another_port: Mutex::new(Some(dst_port)),
            my_next_seq: Mutex::new(0),
            state: Mutex::new(TcpSocketState::PendingSynToBeSent),
        }
    }
    pub fn self_ip(&self) -> Option<IpV4Addr> {
        *self.self_ip.lock()
    }
    pub fn self_port(&self) -> Option<u16> {
        *self.self_port.lock()
    }
    pub fn set_self_port(&self, port: u16) {
        *self.self_port.lock() = Some(port)
    }
    pub fn another_ip(&self) -> Option<IpV4Addr> {
        *self.another_ip.lock()
    }
    pub fn another_port(&self) -> Option<u16> {
        *self.another_port.lock()
    }
    #[allow(clippy::too_many_arguments)]
    fn gen_tcp_packet(
        to_ip: IpV4Addr,
        to_port: u16,
        from_ip: IpV4Addr,
        from_port: u16,
        seq: u32,
        seq_to_ack: Option<u32>,
        syn: bool,
        fin: bool,
        tcp_payload_data: &[u8],
    ) -> Result<Vec<u8>> {
        let eth = EthernetHeader::new(
            EthernetAddr::zero(),
            EthernetAddr::zero(),
            EthernetType::ip_v4(),
        );
        let ipv4_packet = IpV4Packet::new(
            eth,
            to_ip,
            from_ip,
            IpV4Protocol::tcp(),
            size_of::<TcpPacket>() - size_of::<IpV4Packet>(),
        );
        let mut out_tcp = TcpPacket::default();
        out_tcp.set_header_len_nibble(5);
        out_tcp.ip = ipv4_packet;

        out_tcp.ip.set_src(from_ip);
        out_tcp.ip.set_dst(to_ip);

        out_tcp.set_src_port(from_port);
        out_tcp.set_dst_port(to_port);

        out_tcp.set_seq_num(seq);

        out_tcp.set_window(0xffff);
        if let Some(seq_to_ack) = seq_to_ack {
            out_tcp.set_ack();
            out_tcp.set_ack_num(seq_to_ack);
        }
        if syn {
            out_tcp.set_syn();
        }
        if fin {
            out_tcp.set_fin();
        }

        let ip_data_size = out_tcp.header_len() + tcp_payload_data.len();
        out_tcp.ip.set_data_length(ip_data_size);
        let mut out_bytes = vec![0; (size_of::<IpV4Packet>() + ip_data_size).next_multiple_of(2)];
        out_bytes[0..size_of::<TcpPacket>()].copy_from_slice(out_tcp.as_slice());
        out_bytes[size_of::<TcpPacket>()..][..tcp_payload_data.len()]
            .copy_from_slice(tcp_payload_data);
        let mut csum = InternetChecksumGenerator::new();
        csum.feed(&out_bytes[size_of::<IpV4Packet>()..][..ip_data_size]);
        {
            let out_tcp = TcpPacket::from_slice_mut(&mut out_bytes)?;
            csum.feed(out_tcp.ip.src().as_slice());
            csum.feed(out_tcp.ip.dst().as_slice());
            csum.feed(&[0x00, out_tcp.ip.protocol().0]);
            csum.feed(&(ip_data_size as u16).to_be_bytes()); // TCP Header + TCP Data size
            out_tcp.csum = csum.checksum();
        }
        Ok(out_bytes)
    }
    pub fn handle_rx(&self, in_bytes: &[u8]) -> Result<()> {
        let in_packet = Vec::from(in_bytes);
        let in_tcp = TcpPacket::from_slice(&in_packet)?;
        let in_tcp_data = &in_packet[(size_of::<IpV4Packet>() + in_tcp.header_len())..]
            [..(in_tcp.ip.data_length() - in_tcp.header_len())];
        info!("net: tcp: recv: {in_tcp:?}",);
        let from_ip = in_tcp.ip.dst();
        let to_ip = in_tcp.ip.src();
        let from_port = in_tcp.dst_port();
        let to_port = in_tcp.src_port();
        //
        let seq = *self.my_next_seq.lock();
        let mut seq_to_ack = None;
        let mut fin = false;
        let mut syn = false;
        let mut tcp_payload_data = Vec::new();
        let prev_state = *self.state.lock();
        match prev_state {
            TcpSocketState::Listen => {
                if !in_tcp.is_syn() {
                    warn!("net: tcp: recv: unexpected non-SYN received while in TcpSocketState::Listen: {in_tcp:?}");
                    return Ok(());
                }
                info!("net: tcp: recv: TCP SYN received");
                seq_to_ack = Some(in_tcp.seq_num().wrapping_add(1));
                // SYN consumes 1 byte in the seq number space.
                syn = true;
                *self.my_next_seq.lock() = seq.wrapping_add(1);
                *self.state.lock() = TcpSocketState::SynReceived;
            }
            TcpSocketState::SynReceived => {
                if !in_tcp.is_ack() || in_tcp.ack_num() != (*self.my_next_seq.lock()) {
                    warn!("net: tcp: recv: unexpected packet received while in TcpSocketState::SynReceived: {in_tcp:?}");
                }
                *self.state.lock() = TcpSocketState::Established;
                info!("net: tcp: recv: TCP connection established");
                return Ok(());
            }
            TcpSocketState::Established => {
                if in_tcp.is_fin() {
                    seq_to_ack = Some(in_tcp.seq_num().wrapping_add(1));
                    // FIN consumes 1 byte in the seq number space.
                    fin = true;
                    *self.state.lock() = TcpSocketState::LastAck;
                } else if in_tcp_data.is_empty() {
                    return Ok(());
                } else {
                    if let Ok(s) = core::str::from_utf8(in_tcp_data) {
                        info!(
                            "net: tcp: recv: data(str) size = {}: {s}",
                            in_tcp_data.len()
                        );
                    }
                    seq_to_ack = Some(in_tcp.seq_num().wrapping_add(in_tcp_data.len() as u32));
                    tcp_payload_data.extend_from_slice(in_tcp_data);
                    *self.my_next_seq.lock() = seq.wrapping_add(in_tcp_data.len() as u32);
                }
            }
            TcpSocketState::LastAck => {
                if in_tcp.is_ack() {
                    info!("net: tcp: recv: TCP connection closed");
                    // Return to Listen state
                    *self.state.lock() = TcpSocketState::Listen;
                    return Ok(());
                }
            }
            _ => {
                warn!("handler for {prev_state:?} is unimplemented. Skipping...");
                return Ok(());
            }
        }

        //
        let out_bytes = Self::gen_tcp_packet(
            to_ip,
            to_port,
            from_ip,
            from_port,
            seq,
            seq_to_ack,
            syn,
            fin,
            &tcp_payload_data,
        )?;
        Network::take().send_ip_packet(out_bytes.into_boxed_slice());
        Ok(())
    }
}
