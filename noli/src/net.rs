extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::mem::Sliceable;
use crate::print::hexdump;
use crate::println;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::min;
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
unsafe impl Sliceable for IpV4Addr {}

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

static FAKE_TCP_DATA: &str = r#"
HTTP/1.1 200 OK
Content-Type: text/html; charset=UTF-8
Date: Mon, 15 Jan 2024 10:05:49 GMT
Content-Length: 433

<!doctype html>
<html>
<head>
    <title>Example Domain Response</title>
    <meta charset="utf-8" />
</head>
<body>
<div>
    <h1>Example Domain Response</h1>
    <p>This domain is for use in illustrative examples in documents. You may use this
    domain in literature without prior coordination or asking for permission.</p>
    <p><a href="https://www.iana.org/domains/example">More information...</a></p>
</div>
</body>
</html>
"#;

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
        Ok(buf.len())
    }
    pub fn flush(&mut self) -> Result<()> {
        // There's no core::io::Write trait so implement this directly.
        // Do nothing for now.
        println!("TcpStream::flush: {self:?}");
        Ok(())
    }
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        // There's no core::io::Read trait so implement this directly.
        println!("TcpStream::read: {self:?}");
        let copy_size = min(buf.len(), FAKE_TCP_DATA.len());
        buf[..copy_size].copy_from_slice(&FAKE_TCP_DATA.as_bytes()[..copy_size]);
        Ok(copy_size)
    }
}

#[derive(Debug, Clone)]
pub enum DnsResponseEntry {
    A { name: String, addr: IpV4Addr },
}

pub fn lookup_host(host: &str) -> Result<Vec<IpV4Addr>> {
    println!("noli::net::lookup_host: Fake lookup for {host}");
    let addrs = if host == "example.com" {
        vec![IpV4Addr::new([127, 0, 0, 1])]
    } else if host == "example.invalid" {
        // c.f. https://www.rfc-editor.org/rfc/rfc6761.html
        // >  The domain "invalid." and any names falling within ".invalid." are special in the ways listed below.
        // > Users MAY assume that queries for "invalid" names will always return NXDOMAIN responses.
        // > Name resolution APIs and libraries SHOULD recognize "invalid" names as special and SHOULD always return immediate negative responses.
        return Err(Error::Failed("NXDOMAIN"));
    } else {
        Vec::new()
    };

    Ok(addrs)
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
    #[test]
    fn lookup_example_com() {
        let addrs = lookup_host("example.com").expect("lookup_host should succeeds");
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
