extern crate alloc;

use crate::slice::Sliceable;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use core::mem::size_of;

// EtherType field of an Ethernet II frame, stored in network byte order.
#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct EthernetType {
    value: [u8; 2],
}
impl EthernetType {
    pub const fn ip_v4() -> Self {
        Self {
            value: [0x08, 0x00],
        }
    }
    pub const fn arp() -> Self {
        Self {
            value: [0x08, 0x06],
        }
    }
}
impl Debug for EthernetType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "EthernetType({:#04X},{:#04X})",
            self.value[0], self.value[1]
        )
    }
}

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Ord, PartialOrd)]
pub struct EthernetAddr {
    mac: [u8; 6],
}
impl EthernetAddr {
    pub const fn new(mac: [u8; 6]) -> Self {
        Self { mac }
    }
    pub const fn broadcast() -> Self {
        Self {
            mac: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        }
    }
    pub const fn zero() -> Self {
        Self {
            mac: [0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        }
    }
}
impl Debug for EthernetAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.mac[0],
            self.mac[1],
            self.mac[2],
            self.mac[3],
            self.mac[4],
            self.mac[5],
        )
    }
}
impl Display for EthernetAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct EthernetHeader {
    dst: EthernetAddr,
    src: EthernetAddr,
    eth_type: EthernetType,
}
const _: () = assert!(size_of::<EthernetHeader>() == 14);
impl EthernetHeader {
    pub const fn new(
        dst: EthernetAddr,
        src: EthernetAddr,
        eth_type: EthernetType,
    ) -> Self {
        Self { dst, src, eth_type }
    }
    pub fn src(&self) -> EthernetAddr {
        self.src
    }
    pub fn dst(&self) -> EthernetAddr {
        self.dst
    }
    pub fn eth_type(&self) -> EthernetType {
        self.eth_type
    }
}
unsafe impl Sliceable for EthernetHeader {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn ethernet_header_size_is_14() {
        assert_eq!(size_of::<EthernetHeader>(), 14);
    }

    #[test_case]
    fn ethernet_addr_broadcast_is_all_ff() {
        let bytes = EthernetAddr::broadcast().as_slice().to_vec();
        assert_eq!(bytes, [0xFF; 6]);
    }

    #[test_case]
    fn ethernet_addr_zero_is_all_zero() {
        let bytes = EthernetAddr::zero().as_slice().to_vec();
        assert_eq!(bytes, [0x00; 6]);
    }

    #[test_case]
    fn ethernet_type_constants_use_network_byte_order() {
        assert_eq!(EthernetType::ip_v4().as_slice().to_vec(), [0x08, 0x00]);
        assert_eq!(EthernetType::arp().as_slice().to_vec(), [0x08, 0x06]);
    }

    #[test_case]
    fn ethernet_header_layout() {
        let dst = EthernetAddr::new([1, 2, 3, 4, 5, 6]);
        let src = EthernetAddr::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let h = EthernetHeader::new(dst, src, EthernetType::ip_v4());
        let bytes = h.as_slice().to_vec();
        assert_eq!(bytes[0..6], [1, 2, 3, 4, 5, 6]);
        assert_eq!(bytes[6..12], [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        assert_eq!(bytes[12..14], [0x08, 0x00]);
    }
}

unsafe impl Sliceable for EthernetAddr {}
unsafe impl Sliceable for EthernetType {}
