extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::net::checksum::InternetChecksum;
use crate::net::eth::EthernetHeader;
use crate::util::Sliceable;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use alloc::vec::Vec;
use core::mem::size_of;
use core::str::FromStr;

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
#[repr(transparent)]
#[allow(unused)]
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct IpV4Addr([u8; 4]);
impl IpV4Addr {
    pub fn new(ip: [u8; 4]) -> Self {
        Self(ip)
    }
    pub fn network_prefix(&self, mask: IpV4Addr) -> IpV4Addr {
        Self((u32::from_be_bytes(self.0) & u32::from_be_bytes(mask.0)).to_be_bytes())
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
impl FromStr for IpV4Addr {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        const REASON: Error = Error::Failed("Invalid IpV4 address format");
        let s = s
            .split('.')
            .collect::<Vec<&str>>()
            .iter()
            .map(|s| u8::from_str(s).or(Err(REASON)))
            .collect::<Result<Vec<u8>>>()
            .or(Err(REASON))?;
        if s.len() != 4 {
            Err(REASON)
        } else {
            let mut values = [0u8; 4];
            values.copy_from_slice(&s);
            Ok(Self(values))
        }
    }
}
unsafe impl Sliceable for IpV4Addr {}

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
        size = (size + 1) & !1; // make size odd
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
