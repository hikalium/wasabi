extern crate alloc;

use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use core::mem::size_of;
use noli::mem::Sliceable;

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
    pub fn new(mac: [u8; 6]) -> Self {
        Self { mac }
    }
    pub const fn broardcast() -> Self {
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
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5],
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
    pub fn new(dst: EthernetAddr, src: EthernetAddr, eth_type: EthernetType) -> Self {
        Self { dst, src, eth_type }
    }
    pub fn eth_type(&self) -> EthernetType {
        self.eth_type
    }
}
unsafe impl Sliceable for EthernetHeader {}
