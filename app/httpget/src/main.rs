#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use alloc::format;
use alloc::vec::Vec;
use noli::args;
use noli::entry_point;
use noli::error::Result;
use noli::net::lookup_host;
use noli::net::SocketAddr;
use noli::net::TcpStream;
use noli::prelude::*;
use noli::println;

fn main() -> Result<()> {
    let args = args::from_env();
    if args.len() <= 1 {
        println!("Usage: httpget <host>");
        return Ok(());
    }
    let host = &args[1];

    let ip = match lookup_host(host) {
        Ok(results) => {
            if let Some(host) = results.first() {
                *host
            } else {
                Api::exit(1);
            }
        }
        e => {
            println!("  {e:?}");
            Api::exit(1);
        }
    };
    let port = 80;
    let socket_addr: SocketAddr = (ip, port).into();
    let mut stream = TcpStream::connect(socket_addr)?;
    println!("stream: {stream:?}");
    let bytes_written =
        stream.write(format!("GET / HTTP/1.1\nHost: {host}\nConnection: Close\n\n").as_bytes())?;
    println!("bytes_written = {bytes_written}");
    let mut received = Vec::new();
    loop {
        let mut buf = [0u8; 4096];
        let bytes_read = stream.read(&mut buf)?;
        println!("bytes_read = {bytes_read}");
        if bytes_read == 0 {
            break;
        }
        received.extend_from_slice(&buf[..bytes_read]);
        if let Ok(received) = core::str::from_utf8(&received) {
            println!("{received}");
        }
    }
    Ok(())
}

entry_point!(main);
