extern crate alloc;

use crate::checksum::InternetChecksum;
use crate::ip::IpV4Packet;
use crate::slice::Sliceable;
use core::mem::size_of;

#[repr(transparent)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct IcmpType(pub u8);
impl IcmpType {
    pub const fn echo_request() -> Self {
        Self(8)
    }
    pub const fn echo_reply() -> Self {
        Self(0)
    }
}

// ICMP fixed header for Echo Request/Reply, prefixed by the IPv4 packet
// (which itself prefixes the Ethernet header). Echo data follows the 8
// header bytes and is variable-length.
#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct IcmpPacket {
    pub ip: IpV4Packet,
    icmp_type: IcmpType,
    code: u8,
    csum: InternetChecksum,
    identifier: [u8; 2],
    sequence: [u8; 2],
}
const _: () = assert!(size_of::<IcmpPacket>() - size_of::<IpV4Packet>() == 8);
unsafe impl Sliceable for IcmpPacket {}

impl IcmpPacket {
    pub fn icmp_type(&self) -> IcmpType {
        self.icmp_type
    }
    pub fn is_echo_request(&self) -> bool {
        self.icmp_type() == IcmpType::echo_request()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn icmp_packet_extra_size_is_8() {
        assert_eq!(size_of::<IcmpPacket>() - size_of::<IpV4Packet>(), 8);
    }
}
