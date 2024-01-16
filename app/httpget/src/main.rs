#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use noli::entry_point;
use noli::error::Result;
use noli::net::IpV4Addr;
use noli::net::SocketAddr;
use noli::net::TcpStream;
use noli::print::hexdump;
use noli::println;

// printf "\nGET / HTTP/1.1\nHost: example.com\n\n" | nc example.com 80
static SAMPLE_HTTP_GET_REQUEST: &str = "
GET / HTTP/1.1
Host: example.com

";

fn main() -> Result<()> {
    // c.f. https://github.com/d0iasm/toybr/blob/main/net/std/src/http.rs
    println!("httpget");
    let ip_addr = IpV4Addr::new([127, 0, 0, 1]);
    let port = 80;
    let socket_addr: SocketAddr = (ip_addr, port).into();
    let mut stream = TcpStream::connect(socket_addr)?;
    let bytes_written = stream.write(SAMPLE_HTTP_GET_REQUEST.as_bytes())?;
    println!("bytes_written = {bytes_written}");
    let mut buf = [0u8; 4096];
    let bytes_read = stream.read(&mut buf)?;
    let data = &buf[..bytes_read];
    println!("bytes_read = {bytes_read}");
    hexdump(data);
    Ok(())
}

entry_point!(main);
