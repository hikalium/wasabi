extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use alloc::vec::Vec;
use core::str::FromStr;

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

/// Socket is an abstruction of "connection" between two components.
pub enum SocketAddr {
    IpV4 { addr: IpV4Addr, port: u16 },
}
impl SocketAddr {
    pub fn from(addr: IpV4Addr, port: u16) -> Self {
        Self::IpV4 { addr, port }
    }
}

pub struct TcpStream {}
impl TcpStream {}

#[cfg(test)]
mod test {
    #[test]
    fn create_socket_addr() {}
}
