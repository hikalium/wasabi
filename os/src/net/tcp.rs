extern crate alloc;

use crate::net::checksum::InternetChecksum;
use crate::net::ip::IpV4Packet;
use crate::util::Sliceable;
use alloc::fmt;
use alloc::fmt::Debug;

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
