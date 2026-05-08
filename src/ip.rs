extern crate alloc;

use crate::slice::Sliceable;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;

#[repr(transparent)]
#[allow(unused)]
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct IpV4Addr([u8; 4]);
impl IpV4Addr {
    pub const fn new(ip: [u8; 4]) -> Self {
        Self(ip)
    }
    pub fn bytes(&self) -> [u8; 4] {
        self.0
    }
    pub const fn broadcast() -> Self {
        Self([0xff, 0xff, 0xff, 0xff])
    }
}
impl Display for IpV4Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}
impl Debug for IpV4Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(self, f)
    }
}
unsafe impl Sliceable for IpV4Addr {}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test_case]
    fn ipv4_addr_display() {
        let addr = IpV4Addr::new([10, 10, 10, 83]);
        assert_eq!(format!("{addr}"), "10.10.10.83");
    }

    #[test_case]
    fn ipv4_addr_bytes_round_trip() {
        let addr = IpV4Addr::new([1, 2, 3, 4]);
        assert_eq!(addr.bytes(), [1, 2, 3, 4]);
    }

    #[test_case]
    fn ipv4_addr_as_slice_is_be() {
        let addr = IpV4Addr::new([10, 10, 10, 83]);
        assert_eq!(addr.as_slice().to_vec(), [10, 10, 10, 83]);
    }
}
