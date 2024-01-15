#![no_std]
#![no_main]

extern crate alloc;
use noli::entry_point;
use noli::error::Result;
use noli::net::IpV4Addr;
use noli::net::SocketAddr;
use noli::net::TcpStream;
use noli::println;

fn main() -> Result<()> {
    println!("httpget");
    let ip_addr = IpV4Addr::new([127, 0, 0, 1]);
    let port = 80;
    let socket_addr: SocketAddr = (ip_addr, port).into();
    let mut stream = TcpStream::connect(socket_addr)?;
    let bytes_written = stream.write("hello".as_bytes())?;
    println!("bytes_written = {bytes_written}");
    Ok(())
}

entry_point!(main);
