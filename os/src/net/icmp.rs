extern crate alloc;

use crate::net::checksum::InternetChecksum;
use crate::net::ip::IpV4Packet;

#[repr(transparent)]
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct IcmpType(u8);
impl IcmpType {
    pub fn reply() -> Self {
        Self(0)
    }
    pub fn request() -> Self {
        Self(8)
    }
}

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct IcmpPacket {
    ip: IpV4Packet,
    icmp_type: IcmpType,
    code: u8,
    csum: InternetChecksum,
    identifier: [u8; 2],
    sequence: [u8; 2],
}
