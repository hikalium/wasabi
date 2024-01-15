extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::print::hexdump;
use crate::println;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use alloc::vec::Vec;
use core::convert::From;
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
#[derive(Debug)]
pub enum SocketAddr {
    IpV4 { addr: IpV4Addr, port: u16 },
}
impl From<(IpV4Addr, u16)> for SocketAddr {
    fn from(addr: (IpV4Addr, u16)) -> Self {
        let (addr, port) = addr;
        Self::IpV4 { addr, port }
    }
}

#[derive(Debug)]
pub struct TcpStream {
    addr: SocketAddr,
}
impl TcpStream {
    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }
    pub fn connect(addr: SocketAddr) -> Result<Self> {
        Ok(Self { addr })
    }
    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        // There's no core::io::Write trait so implement this directly.
        println!("TcpStream::write: {self:?}");
        hexdump(buf);
        Ok(0)
    }
    pub fn flush(&mut self) -> Result<()> {
        // There's no core::io::Write trait so implement this directly.
        // Do nothing for now.
        println!("TcpStream::flush: {self:?}");
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod test {
    use super::*;
    #[test]
    fn create_socket_addr() {
        let ip_addr = IpV4Addr::new([127, 0, 0, 1]);
        let port = 80;
        let socket_addr: SocketAddr = (ip_addr, port).into();
        assert!(TcpStream::connect(socket_addr).is_ok());
    }
}
