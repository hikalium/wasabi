extern crate alloc;

use crate::net::eth::EthernetAddr;
use crate::net::eth::EthernetHeader;
use crate::net::eth::EthernetType;
use alloc::fmt;
use alloc::fmt::Debug;
use core::mem::size_of;
use noli::mem::Sliceable;
use noli::net::IpV4Addr;

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct ArpPacket {
    eth_header: EthernetHeader,
    hw_type: [u8; 2],
    proto_type: [u8; 2],
    hw_addr_size: u8,
    proto_addr_size: u8,
    op: [u8; 2],
    sender_mac: EthernetAddr,
    sender_ip: IpV4Addr,
    target_mac: EthernetAddr,
    target_ip: IpV4Addr,
}
const _: () = assert!(size_of::<ArpPacket>() == 42);
impl ArpPacket {
    pub fn is_response(&self) -> bool {
        self.op == [0x00, 0x02]
    }
    pub fn sender_eth_addr(&self) -> EthernetAddr {
        self.sender_mac
    }
    pub fn sender_ip_addr(&self) -> IpV4Addr {
        self.sender_ip
    }
    pub fn request(src_eth: EthernetAddr, src_ip: IpV4Addr, dst_ip: IpV4Addr) -> Self {
        Self {
            eth_header: EthernetHeader::new(
                EthernetAddr::broardcast(),
                src_eth,
                EthernetType::arp(),
            ),
            hw_type: [0x00, 0x01],
            proto_type: [0x08, 0x00],
            hw_addr_size: 6,
            proto_addr_size: 4,
            op: [0x00, 0x01],
            sender_mac: src_eth,
            sender_ip: src_ip,
            target_mac: EthernetAddr::zero(), // target_mac = Unknown
            target_ip: dst_ip,
        }
    }
}
impl Debug for ArpPacket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ArpPacket {{ op: {}, sender: {:?} ({:?}), target: {:?} ({:?}) }}",
            match (self.op[0], self.op[1]) {
                (0, 1) => "request",
                (0, 2) => "response",
                (_, _) => "unknown",
            },
            self.sender_ip,
            self.sender_mac,
            self.target_ip,
            self.target_mac,
        )
    }
}
unsafe impl Sliceable for ArpPacket {}
