extern crate alloc;

use crate::net::checksum::InternetChecksum;
use crate::net::eth::EthernetHeader;
use alloc::fmt::Debug;
use core::mem::size_of;
use noli::mem::Sliceable;
use noli::net::IpV4Addr;

#[repr(transparent)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct IpV4Protocol(pub u8);
impl IpV4Protocol {
    pub fn icmp() -> Self {
        Self(1)
    }
    pub fn tcp() -> Self {
        Self(6)
    }
    pub const fn udp() -> Self {
        Self(17)
    }
}

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct IpV4Packet {
    pub eth: EthernetHeader,
    version_and_ihl: u8,
    dscp_and_ecn: u8,
    length: [u8; 2], // byte size including IPv4 header
    ident: u16,
    flags: u16,
    ttl: u8,
    protocol: IpV4Protocol,
    csum: InternetChecksum,
    src: IpV4Addr,
    dst: IpV4Addr,
}
const _: () = assert!(size_of::<IpV4Packet>() - size_of::<EthernetHeader>() == 20);
impl IpV4Packet {
    pub fn src(&self) -> IpV4Addr {
        self.src
    }
    pub fn dst(&self) -> IpV4Addr {
        self.dst
    }
    pub fn new(
        eth: EthernetHeader,
        dst: IpV4Addr,
        src: IpV4Addr,
        protocol: IpV4Protocol,
        data_length: usize,
    ) -> Self {
        let mut this = Self {
            eth,
            version_and_ihl: 0x45, // IPv4, header len = 5 * sizeof(uint32_t) = 20 bytes
            ttl: 0xff,
            protocol,
            src,
            dst,
            ..Self::default()
        };
        this.set_data_length(data_length);
        this
    }
    pub fn set_dst(&mut self, dst: IpV4Addr) {
        self.dst = dst;
    }
    pub fn set_src(&mut self, src: IpV4Addr) {
        self.src = src;
    }
    pub fn protocol(&self) -> IpV4Protocol {
        self.protocol
    }
    pub fn data_length(&self) -> usize {
        self.total_size() - (size_of::<Self>() - size_of::<EthernetHeader>())
    }
    pub fn set_data_length(&mut self, mut size: usize) {
        size += size_of::<Self>() - size_of::<EthernetHeader>(); // IP header size
        self.length = (size as u16).to_be_bytes()
    }
    /// Number of bytes including IPv4 header and its payload
    pub fn total_size(&self) -> usize {
        u16::from_be_bytes(self.length) as usize
    }
    pub fn clear_checksum(&mut self) {
        self.csum = InternetChecksum::default();
    }
    pub fn set_checksum(&mut self, csum: InternetChecksum) {
        self.csum = csum;
    }
}
unsafe impl Sliceable for IpV4Packet {}
