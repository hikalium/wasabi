extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::mem::Sliceable;
use crate::prelude::*;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::From;
use core::str::FromStr;
use sabi::RawIpV4Addr;

#[repr(transparent)]
#[allow(unused)]
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct IpV4Addr([u8; 4]);
impl IpV4Addr {
    pub fn new(ip: [u8; 4]) -> Self {
        Self(ip)
    }
    pub fn bytes(&self) -> [u8; 4] {
        self.0
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
unsafe impl Sliceable for IpV4Addr {}

/// Socket is an abstruction of "connection" between two components.
#[derive(Debug)]
pub struct SocketAddr {
    addr: IpV4Addr,
    port: u16,
}
impl From<(IpV4Addr, u16)> for SocketAddr {
    fn from(addr: (IpV4Addr, u16)) -> Self {
        let (addr, port) = addr;
        Self { addr, port }
    }
}

#[derive(Debug)]
pub struct TcpStream {
    sock_addr: SocketAddr,
    handle: i64,
}
impl TcpStream {
    pub fn sock_addr(&self) -> &SocketAddr {
        &self.sock_addr
    }
    pub fn connect(sa: SocketAddr) -> Result<Self> {
        let handle = Api::open_tcp_socket(sa.addr.0, sa.port);
        if handle >= 0 {
            Ok(Self {
                sock_addr: sa,
                handle,
            })
        } else {
            Err(Error::Failed("Failed to open TCP socket"))
        }
    }
    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        // There's no core::io::Write trait so implement this directly.
        let bytes_written = Api::write_to_tcp_socket(self.handle, buf);
        if bytes_written >= 0 {
            Ok(bytes_written as usize)
        } else {
            let err = bytes_written;
            match err {
                -1 => Err(Error::Failed("NO_SUCH_SOCKET")),
                -2 => Err(Error::Failed("WRITE_ERROR")),
                _ => Err(Error::Failed("UNDEFINED")),
            }
        }
    }
    /// Reads data received so far with this stream up to the size of `buf`.
    /// Returns the size of the data read by this call.
    /// This function blocks the execution until there are some data available.
    /// This function returns 0 once the connection is closed.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        // There's no core::io::Read trait so implement this directly.
        let bytes_written = Api::read_from_tcp_socket(self.handle, buf);
        if bytes_written >= 0 {
            Ok(bytes_written as usize)
        } else {
            let err = bytes_written;
            match err {
                -1 => Err(Error::Failed("NO_SUCH_SOCKET")),
                -2 => Err(Error::Failed("READ_ERROR")),
                _ => Err(Error::Failed("UNDEFINED")),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum DnsResponseEntry {
    A { name: String, addr: IpV4Addr },
}

pub fn lookup_host(host: &str) -> Result<Vec<IpV4Addr>> {
    let mut result = [RawIpV4Addr::default(); 4];
    match Api::nslookup(host, &mut result) {
        n if n >= 0 => Ok(result
            .iter()
            .take(n as usize)
            .map(|e| IpV4Addr::new(*e))
            .collect()),
        -1 => Err(Error::Failed("RESOLUTION_FAILED")),
        -2 => Err(Error::Failed("NXDOMAIN")),
        _ => Err(Error::Failed("UNDEFINED")),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod test {
    use super::*;
    #[test]
    fn create_socket_addr() {
        let ip_addr = IpV4Addr::new([127, 0, 0, 1]);
        let port = 80;
        let sa: SocketAddr = (ip_addr, port).into();
        assert_eq!(sa.addr, ip_addr);
        assert_eq!(sa.port, 80);
    }
    #[test]
    fn lookup_example_com() {
        let addrs = lookup_host("nolitest.example.com").expect("lookup_host should succeeds");
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0], IpV4Addr::new([127, 0, 0, 1]));
    }
    #[test]
    fn lookup_nx_domain() {
        // c.f. https://www.rfc-editor.org/rfc/rfc6761.html
        // >  The domain "invalid." and any names falling within ".invalid." are special in the ways listed below.
        // > Users MAY assume that queries for "invalid" names will always return NXDOMAIN responses.
        // > Name resolution APIs and libraries SHOULD recognize "invalid" names as special and SHOULD always return immediate negative responses.
        assert!(lookup_host("example.invalid").is_err());
    }
}
