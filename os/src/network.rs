extern crate alloc;

use crate::error::Result;
use alloc::fmt;
use alloc::fmt::Debug;
use core::marker::PhantomPinned;
use core::mem::size_of;

#[repr(packed)]
#[derive(Clone, Copy)]
pub struct EthernetAddress {
    mac: [u8; 6],
}
impl EthernetAddress {
    pub fn new(mac: [u8; 6]) -> Self {
        Self { mac }
    }
    const fn broardcast() -> Self {
        Self {
            mac: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        }
    }
    const fn zero() -> Self {
        Self {
            mac: [0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        }
    }
}
impl Debug for EthernetAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5],
        )
    }
}
#[repr(packed)]
pub struct IpV4Addr {
    ip: [u8; 4],
}
impl IpV4Addr {
    pub fn new(ip: [u8; 4]) -> Self {
        Self { ip }
    }
}
impl Debug for IpV4Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.ip[0], self.ip[1], self.ip[2], self.ip[3],
        )
    }
}
#[repr(packed)]
#[allow(unused)]
pub struct EthernetHeader {
    dst_eth: EthernetAddress,
    src_eth: EthernetAddress,
    eth_type: [u8; 2],
}
#[repr(packed)]
#[allow(unused)]
pub struct ArpPacket {
    eth_header: EthernetHeader,
    hw_type: [u8; 2],
    proto_type: [u8; 2],
    hw_addr_size: u8,
    proto_addr_size: u8,
    op: [u8; 2],
    sender_mac: EthernetAddress,
    sender_ip: IpV4Addr,
    target_mac: EthernetAddress,
    target_ip: IpV4Addr,
    //
    _pinned: PhantomPinned,
}
impl ArpPacket {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // SAFETY: any bytes can be parsed as the ARP packet
        if bytes.len() >= size_of::<ArpPacket>() {
            let mut tmp = [0u8; size_of::<ArpPacket>()];
            tmp.copy_from_slice(&bytes[0..size_of::<ArpPacket>()]);
            Ok(unsafe { core::mem::transmute(tmp) })
        } else {
            Err(crate::error::Error::Failed("too short"))
        }
    }
    pub fn request(src_eth: EthernetAddress, src_ip: IpV4Addr, dst_ip: IpV4Addr) -> Self {
        Self {
            eth_header: EthernetHeader {
                dst_eth: src_eth,
                src_eth: EthernetAddress::broardcast(),
                eth_type: [0x08, 0x06],
            },
            hw_type: [0x00, 0x01],
            proto_type: [0x08, 0x00],
            hw_addr_size: 6,
            proto_addr_size: 4,
            op: [0x00, 0x01],
            sender_mac: src_eth,
            sender_ip: src_ip,
            target_mac: EthernetAddress::zero(), // target_mac = Unknown
            target_ip: dst_ip,
            //
            _pinned: PhantomPinned,
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
const _: () = assert!(size_of::<ArpPacket>() == 42);
