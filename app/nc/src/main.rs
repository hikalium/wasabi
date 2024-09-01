#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use noli::args;
use noli::net::lookup_host;
use noli::prelude::*;

fn main() -> Result<()> {
    let args = args::from_env();
    if args.len() <= 2 {
        println!("Usage: nc <host> <port>");
        return Ok(());
    }
    let host = &args[1];
    let port = &args[2];

    let host = match lookup_host(host) {
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
    println!("Connecting to {host}:{port}...");
    Ok(())
}
entry_point!(main);
