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
use crate::util::Sliceable;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::vec::Vec;
use core::mem::size_of;

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
#[derive(Debug)]
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

pub struct TcpSocket {
    state: Mutex<TcpSocketState>,
}
impl Debug for TcpSocket {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "TcpSocket{{ state: {:?} }}", *self.state.lock())
    }
}
impl TcpSocket {
    pub fn new(state: TcpSocketState) -> Self {
        Self {
            state: Mutex::new(state, "TcpSocket::state"),
        }
    }
    pub fn handle_rx(&self, in_bytes: &[u8]) -> Result<()> {
        let in_packet = Vec::from(in_bytes);
        let in_tcp = TcpPacket::from_slice(&in_packet)?;
        info!("net: rx: in : {in_tcp:?}",);
        let data = &in_packet[(size_of::<IpV4Packet>() + in_tcp.header_len())..]
            [..(in_tcp.ip.payload_size() - in_tcp.header_len())];
        if !data.is_empty() {
            info!("net: rx: TCP: data: {data:X?}");
            if let Ok(s) = core::str::from_utf8(data) {
                info!("net: rx: TCP: data(str): {s}");
            }
        }
        {
            let mut out_bytes = Vec::new();
            {
                let from_ip = in_tcp.ip.dst();
                let to_ip = in_tcp.ip.src();
                let from_port = in_tcp.dst_port();
                let to_port = in_tcp.src_port();

                out_bytes.resize(size_of::<TcpPacket>() + data.len(), 0);
                out_bytes[size_of::<TcpPacket>()..].copy_from_slice(data);
                let eth = EthernetHeader::new(
                    EthernetAddr::zero(),
                    EthernetAddr::zero(),
                    EthernetType::ip_v4(),
                );
                let ip_data_length = out_bytes.len() - size_of::<IpV4Packet>();
                let ipv4_packet =
                    IpV4Packet::new(eth, to_ip, from_ip, IpV4Protocol::tcp(), ip_data_length);
                let out_tcp = TcpPacket::from_slice_mut(&mut out_bytes)?;
                out_tcp.set_header_len_nibble(5);
                out_tcp.ip = ipv4_packet;

                out_tcp.ip.set_src(from_ip);
                out_tcp.ip.set_dst(to_ip);

                out_tcp.set_src_port(from_port);
                out_tcp.set_dst_port(to_port);

                if in_tcp.is_syn() {
                    out_tcp.set_syn();
                    out_tcp.set_ack();
                    out_tcp.set_ack_num(in_tcp.seq_num().wrapping_add(1));
                    out_tcp.set_seq_num(0);
                } else if in_tcp.is_fin() {
                    out_tcp.set_fin();
                    out_tcp.set_ack();
                    out_tcp.set_ack_num(in_tcp.seq_num().wrapping_add(1));
                    out_tcp.set_seq_num(1);
                } else if in_tcp.ack_num() == 1 {
                    out_tcp.set_ack();
                    out_tcp.set_ack_num(in_tcp.seq_num().wrapping_add(data.len() as u32));
                    out_tcp.set_seq_num(1);
                } else if in_tcp.ack_num() == 2 {
                    info!("TCP connection closed");
                    return Ok(());
                }
                out_tcp.set_ack();
                out_tcp.set_window(0xffff);

                out_tcp.csum = InternetChecksum::default();
            }
            let mut csum = InternetChecksumGenerator::new();
            csum.feed(&out_bytes[size_of::<IpV4Packet>()..]);
            {
                let out_tcp = TcpPacket::from_slice_mut(&mut out_bytes)?;
                csum.feed(out_tcp.ip.src().as_slice());
                csum.feed(out_tcp.ip.dst().as_slice());
                csum.feed(&[0x00, out_tcp.ip.protocol().0]);
                csum.feed(&20u16.to_be_bytes()); // TCP Header + TCP Data size
                out_tcp.csum = csum.checksum();
                info!("net: rx: out: {out_tcp:?}",);
                crate::print::hexdump(out_tcp.as_slice());
            }
            Network::take().send_ip_packet(out_bytes.into_boxed_slice());
        }
        Ok(())
    }
}
