extern crate alloc;

use crate::net::checksum::InternetChecksum;
use crate::net::eth::EthernetHeader;
use crate::net::ip::IpV4Addr;
use crate::net::ip::IpV4Packet;
use crate::net::ip::IpV4Protocol;
use crate::util::Sliceable;
use alloc::fmt;
use alloc::fmt::Debug;
use core::mem::size_of;

#[repr(transparent)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
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
const _: () = assert!(size_of::<IcmpPacket>() - size_of::<IpV4Packet>() == 8);
unsafe impl Sliceable for IcmpPacket {}
impl IcmpPacket {
    pub fn new_request(dst: IpV4Addr) -> Self {
        let ip = IpV4Packet::new(
            EthernetHeader::default(),
            dst,
            IpV4Addr::default(),
            IpV4Protocol::icmp(),
            size_of::<Self>() - size_of::<IpV4Packet>(),
        );
        let mut this = Self {
            ip,
            icmp_type: IcmpType::request(),
            ..Default::default()
        };
        this.csum = InternetChecksum::calc(&this.as_slice()[size_of::<IpV4Packet>()..]);
        this
    }
}
impl Debug for IcmpPacket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ICMP: {:?} -> {:?}: {:?}",
            self.ip.src(),
            self.ip.dst(),
            self.icmp_type
        )
    }
}
