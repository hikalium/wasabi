#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]

extern crate alloc;
use noli::args;
use noli::net::lookup_host;
use noli::prelude::*;

fn main() -> Result<()> {
    let args = args::from_env();
    if args.len() <= 1 {
        println!("Usage: dig <hostname> ...");
        return Ok(());
    }
    for host in &args[1..] {
        println!("{host}:");
        match lookup_host(host) {
            Ok(results) => {
                println!("{} answers:", results.len());
                for r in results {
                    println!("  {r}")
                }
            }
            e => {
                println!("  {e:?}")
            }
        }
    }
    Ok(())
}

entry_point!(main);
