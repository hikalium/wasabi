extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::executor::yield_execution;
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
use alloc::collections::VecDeque;
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
    pub fn is_rst(&self) -> bool {
        (self.flags[1] & (1 << 2)) != 0
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
            "TCP :{} -> :{}, seq = {}, ack = {}, flags = {:#018b}{}{}{}{}",
            self.src_port(),
            self.dst_port(),
            self.seq_num(),
            self.ack_num(),
            self.flags(),
            if self.is_fin() { " FIN" } else { "" },
            if self.is_syn() { " SYN" } else { "" },
            if self.is_rst() { " RST" } else { "" },
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
}

// Step 1: Just receive (no error handling)
// Step 2: Just echo (no error handling)

pub struct TcpSocket {
    self_ip: Mutex<Option<IpV4Addr>>,
    self_port: Mutex<Option<u16>>,
    another_ip: Mutex<Option<IpV4Addr>>,
    another_port: Mutex<Option<u16>>,
    my_next_seq: Mutex<u32>,
    last_seq_to_ack: Mutex<u32>,
    state: Mutex<TcpSocketState>,
    rx_data: Mutex<VecDeque<u8>>,
    tx_data: Mutex<VecDeque<u8>>,
    keep_listening: bool,
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
            last_seq_to_ack: Mutex::new(0),
            state: Mutex::new(TcpSocketState::Listen),
            rx_data: Default::default(),
            tx_data: Default::default(),
            keep_listening: true,
        }
    }
    pub fn new_client(dst_ip: IpV4Addr, dst_port: u16) -> Self {
        Self {
            self_ip: Default::default(),
            self_port: Default::default(),
            another_ip: Mutex::new(Some(dst_ip)),
            another_port: Mutex::new(Some(dst_port)),
            my_next_seq: Mutex::new(0),
            last_seq_to_ack: Mutex::new(0),
            // Syn will be sent in TcpSocket::open()
            state: Mutex::new(TcpSocketState::SynSent),
            rx_data: Default::default(),
            tx_data: Default::default(),
            keep_listening: false,
        }
    }
    pub fn self_ip(&self) -> Option<IpV4Addr> {
        *self.self_ip.lock()
    }
    pub fn set_self_ip(&self, ip: Option<IpV4Addr>) {
        *self.self_ip.lock() = ip
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
    pub fn rx_data(&self) -> &Mutex<VecDeque<u8>> {
        &self.rx_data
    }
    pub fn tx_data(&self) -> &Mutex<VecDeque<u8>> {
        &self.tx_data
    }
    fn gen_syn_packet(
        to_ip: IpV4Addr,
        to_port: u16,
        from_ip: IpV4Addr,
        from_port: u16,
        seq: u32,
    ) -> Result<Vec<u8>> {
        Self::gen_tcp_packet(
            to_ip,
            to_port,
            from_ip,
            from_port,
            seq,
            None,
            true,
            false,
            &[],
        )
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
        info!("net: tcp: send: {out_tcp:?}",);
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
        let mut seq_to_ack = in_tcp.seq_num();
        let mut fin = false;
        let mut syn = false;
        let prev_state = *self.state.lock();
        match prev_state {
            TcpSocketState::Listen => {
                if !in_tcp.is_syn() {
                    warn!("net: tcp: recv: unexpected non-SYN received while in TcpSocketState::Listen: {in_tcp:?}");
                    return Ok(());
                }
                info!("net: tcp: recv: TCP SYN received");
                seq_to_ack = seq_to_ack.wrapping_add(1);
                // SYN consumes 1 byte in the seq number space.
                syn = true;
                *self.my_next_seq.lock() = seq.wrapping_add(1);
                *self.state.lock() = TcpSocketState::SynReceived;
            }
            TcpSocketState::SynSent => {
                // If SYN+ACK is received, reply ACK and transition to Established state
                if !in_tcp.is_syn() || !in_tcp.is_ack() {
                    warn!(
                        "net: tcp: recv: SYN+ACK was expected in {prev_state:?} but got {in_tcp:?}"
                    );
                    return Ok(());
                }
                if in_tcp.ack_num() != (*self.my_next_seq.lock()) {
                    warn!(
                        "net: tcp: recv: unexpected ACK received while in {prev_state:?}: {in_tcp:?}"
                    );
                    return Ok(());
                }
                // Reply ACK
                // SYN consumes 1 byte in the seq number space.
                seq_to_ack = seq_to_ack.wrapping_add(1);
                // Now the socket is established
                *self.state.lock() = TcpSocketState::Established;
                info!("net: tcp: recv: TCP connection established");
            }
            TcpSocketState::SynReceived => {
                if !in_tcp.is_ack() || in_tcp.ack_num() != (*self.my_next_seq.lock()) {
                    warn!(
                        "net: tcp: recv: unexpected packet received while in {prev_state:?}: {in_tcp:?}"
                    );
                    return Ok(());
                }
                *self.state.lock() = TcpSocketState::Established;
                info!("net: tcp: recv: TCP connection established");
                *self.another_ip.lock() = Some(to_ip);
                *self.another_port.lock() = Some(to_port);
                *self.self_ip.lock() = Some(from_ip);
                *self.self_port.lock() = Some(from_port);
                return Ok(());
            }
            TcpSocketState::Established => {
                if in_tcp.is_fin() {
                    seq_to_ack = seq_to_ack.wrapping_add(1);
                    // FIN consumes 1 byte in the seq number space.
                    fin = true;
                    *self.state.lock() = TcpSocketState::LastAck;
                }
                seq_to_ack = seq_to_ack.wrapping_add(in_tcp_data.len() as u32);
                self.rx_data.lock().extend(in_tcp_data);
                // Send ACK
            }
            TcpSocketState::LastAck => {
                if in_tcp.is_ack() {
                    info!("net: tcp: recv: TCP connection closed");
                    if self.keep_listening {
                        *self.state.lock() = TcpSocketState::Listen;
                    } else {
                        *self.state.lock() = TcpSocketState::Closed;
                    }
                    return Ok(());
                }
            }
            _ => {
                warn!("handler for {prev_state:?} is unimplemented. Skipping...");
                return Ok(());
            }
        }

        //
        *self.last_seq_to_ack.lock() = seq_to_ack;
        let out_bytes = Self::gen_tcp_packet(
            to_ip,
            to_port,
            from_ip,
            from_port,
            seq,
            Some(seq_to_ack),
            syn,
            fin,
            &[],
        )?;
        Network::take().send_ip_packet(out_bytes.into_boxed_slice());
        Ok(())
    }
    pub fn poll_tx(&self) -> Result<()> {
        if self.tx_data.lock().is_empty() || !self.is_established() {
            return Ok(());
        }
        let to_ip = self
            .another_ip()
            .ok_or(Error::Failed("another_ip should be populated"))?;
        let to_port = self
            .another_port()
            .ok_or(Error::Failed("another_port should be populated"))?;
        let from_ip = self
            .self_ip()
            .ok_or(Error::Failed("self_ip should be populated"))?;
        let from_port = self
            .self_port()
            .ok_or(Error::Failed("self_port should be populated"))?;
        //
        let seq = *self.my_next_seq.lock();
        let tcp_data_to_send = Vec::from_iter(self.tx_data.lock().drain(..));
        info!("Trying to send data {tcp_data_to_send:?} to {to_ip}:{to_port}");
        let prev_state = *self.state.lock();
        match prev_state {
            TcpSocketState::Established => {
                *self.my_next_seq.lock() = seq.wrapping_add(tcp_data_to_send.len() as u32);
            }
            _ => {
                return Err(Error::Failed("The socket is not ready"));
            }
        }

        let seq_to_ack = *self.last_seq_to_ack.lock();
        let fin = false;
        let syn = false;
        let out_bytes = Self::gen_tcp_packet(
            to_ip,
            to_port,
            from_ip,
            from_port,
            seq,
            Some(seq_to_ack),
            syn,
            fin,
            &tcp_data_to_send,
        )?;
        Network::take().send_ip_packet(out_bytes.into_boxed_slice());
        Ok(())
    }
    pub fn open(&self) -> Result<()> {
        let to_ip = self
            .another_ip()
            .ok_or(Error::Failed("another_ip should be populated"))?;
        let to_port = self
            .another_port()
            .ok_or(Error::Failed("another_port should be populated"))?;
        let from_ip = self
            .self_ip()
            .ok_or(Error::Failed("self_ip should be populated"))?;
        let from_port = self
            .self_port()
            .ok_or(Error::Failed("self_port should be populated"))?;
        info!("Trying to open a socket with {to_ip}:{to_port}");
        let seq = 1234;
        let syn_packet = Self::gen_syn_packet(to_ip, to_port, from_ip, from_port, seq)?;
        *self.my_next_seq.lock() = seq.wrapping_add(1);
        Network::take().send_ip_packet(syn_packet.into_boxed_slice());
        Ok(())
    }
    pub async fn wait_until_connection_is_established(&self) {
        while *self.state.lock() != TcpSocketState::Established {
            yield_execution().await;
        }
    }
    pub async fn wait_on_rx(&self) {
        while *self.state.lock() == TcpSocketState::Established && self.rx_data.lock().is_empty() {
            yield_execution().await;
        }
    }
    pub fn is_established(&self) -> bool {
        *self.state.lock() == TcpSocketState::Established
    }
    pub fn is_trying_to_connect(&self) -> bool {
        matches!(
            *self.state.lock(),
            TcpSocketState::SynSent | TcpSocketState::SynReceived
        )
    }
}
