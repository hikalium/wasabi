extern crate alloc;

use crate::checksum::InternetChecksum;
use crate::checksum::InternetChecksumGenerator;
use crate::ip::IpV4Addr;
use crate::ip::IpV4Packet;
use crate::slice::Sliceable;
use core::mem::size_of;

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
