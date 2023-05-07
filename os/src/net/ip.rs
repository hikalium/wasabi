extern crate alloc;

use crate::net::checksum::InternetChecksum;
use crate::net::eth::EthernetHeader;
use crate::util::Sliceable;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use core::mem::size_of;

#[repr(transparent)]
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct IpV4Protocol(u8);
impl IpV4Protocol {
    /*
    pub fn icmp() -> Self {
        Self(1)
    }
    pub fn tcp() -> Self {
        Self(6)
    }
    */
    pub const fn udp() -> Self {
        Self(17)
    }
}
#[repr(transparent)]
#[allow(unused)]
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct IpV4Addr([u8; 4]);
impl IpV4Addr {
    pub fn new(ip: [u8; 4]) -> Self {
        Self(ip)
    }
}
impl Display for IpV4Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3],)
    }
}
impl Debug for IpV4Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3],)
    }
}
impl IpV4Addr {
    pub const fn broardcast() -> Self {
        Self([0xff, 0xff, 0xff, 0xff])
    }
}
unsafe impl Sliceable for IpV4Addr {}
#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct IpV4Packet {
    eth: EthernetHeader,
    version_and_ihl: u8,
    dscp_and_ecn: u8,
    length: [u8; 2],
    ident: u16,
    flags: u16,
    ttl: u8,
    protocol: IpV4Protocol,
    csum: InternetChecksum,
    src: IpV4Addr,
    dst: IpV4Addr,
}
impl IpV4Packet {
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
        this.set_data_length(data_length as u16);
        //this.ident = 0x426b;
        this.calc_checksum();
        this
    }
    pub fn protocol(&self) -> IpV4Protocol {
        self.protocol
    }
    fn set_data_length(&mut self, mut size: u16) {
        size += (size_of::<Self>() - size_of::<EthernetHeader>()) as u16; // IP header size
        size = (size + 1) & !1; // make size odd
        self.length = size.to_be_bytes()
    }
    fn calc_checksum(&mut self) {
        self.csum = InternetChecksum::calc(&self.as_slice()[size_of::<EthernetHeader>()..]);
    }
}
unsafe impl Sliceable for IpV4Packet {}
