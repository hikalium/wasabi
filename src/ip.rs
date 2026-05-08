extern crate alloc;

use crate::checksum::InternetChecksum;
use crate::eth::EthernetHeader;
use crate::slice::Sliceable;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use core::mem::size_of;

#[repr(transparent)]
#[allow(unused)]
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct IpV4Addr([u8; 4]);
impl IpV4Addr {
    pub const fn new(ip: [u8; 4]) -> Self {
        Self(ip)
    }
    pub fn bytes(&self) -> [u8; 4] {
        self.0
    }
    pub const fn broadcast() -> Self {
        Self([0xff, 0xff, 0xff, 0xff])
    }
}
impl Display for IpV4Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}
impl Debug for IpV4Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(self, f)
    }
}
unsafe impl Sliceable for IpV4Addr {}

#[repr(transparent)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct IpV4Protocol(pub u8);
impl IpV4Protocol {
    pub const fn icmp() -> Self {
        Self(1)
    }
}

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct IpV4Packet {
    pub eth: EthernetHeader,
    version_and_ihl: u8, // 0x45 for IPv4 with 20-byte header
    dscp_and_ecn: u8,
    length: [u8; 2], // total IP packet length (header + payload)
    ident: [u8; 2],
    flags: [u8; 2],
    ttl: u8,
    protocol: IpV4Protocol,
    csum: InternetChecksum,
    src: IpV4Addr,
    dst: IpV4Addr,
}
const _: () =
    assert!(size_of::<IpV4Packet>() - size_of::<EthernetHeader>() == 20);
unsafe impl Sliceable for IpV4Packet {}

impl IpV4Packet {
    pub fn new(
        eth: EthernetHeader,
        dst: IpV4Addr,
        src: IpV4Addr,
        protocol: IpV4Protocol,
        payload_length: usize,
    ) -> Self {
        let total =
            (size_of::<Self>() - size_of::<EthernetHeader>()) + payload_length;
        Self {
            eth,
            version_and_ihl: 0x45,
            ttl: 0xff,
            protocol,
            length: (total as u16).to_be_bytes(),
            src,
            dst,
            ..Self::default()
        }
    }
    pub fn src(&self) -> IpV4Addr {
        self.src
    }
    pub fn dst(&self) -> IpV4Addr {
        self.dst
    }
    pub fn protocol(&self) -> IpV4Protocol {
        self.protocol
    }
    pub fn total_length(&self) -> usize {
        u16::from_be_bytes(self.length) as usize
    }
    pub fn payload_length(&self) -> usize {
        self.total_length()
            .saturating_sub(size_of::<Self>() - size_of::<EthernetHeader>())
    }
    pub fn recompute_checksum(&mut self) {
        self.csum = InternetChecksum::default();
        let header_start = size_of::<EthernetHeader>();
        let header_end = size_of::<Self>();
        let csum =
            InternetChecksum::calc(&self.as_slice()[header_start..header_end]);
        self.csum = csum;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test_case]
    fn ipv4_addr_display() {
        let addr = IpV4Addr::new([10, 10, 10, 83]);
        assert_eq!(format!("{addr}"), "10.10.10.83");
    }

    #[test_case]
    fn ipv4_addr_bytes_round_trip() {
        let addr = IpV4Addr::new([1, 2, 3, 4]);
        assert_eq!(addr.bytes(), [1, 2, 3, 4]);
    }

    #[test_case]
    fn ipv4_addr_as_slice_is_be() {
        let addr = IpV4Addr::new([10, 10, 10, 83]);
        assert_eq!(addr.as_slice().to_vec(), [10, 10, 10, 83]);
    }

    #[test_case]
    fn ipv4_packet_header_is_20_bytes() {
        // size minus the embedded EthernetHeader is the IPv4 header alone.
        assert_eq!(size_of::<IpV4Packet>() - size_of::<EthernetHeader>(), 20);
    }

    #[test_case]
    fn ipv4_packet_recompute_checksum_self_check() {
        use crate::checksum::InternetChecksumGenerator;
        let mut pkt = IpV4Packet::new(
            EthernetHeader::default(),
            IpV4Addr::new([192, 168, 1, 1]),
            IpV4Addr::new([192, 168, 1, 2]),
            IpV4Protocol::icmp(),
            8,
        );
        pkt.recompute_checksum();
        // Standard self-check: feeding the header (with its csum field
        // populated) back through the generator yields 0x0000.
        let header = &pkt.as_slice()
            [size_of::<EthernetHeader>()..size_of::<IpV4Packet>()];
        let mut g = InternetChecksumGenerator::new();
        g.feed(header);
        assert_eq!(g.checksum(), InternetChecksum::default());
    }
}
