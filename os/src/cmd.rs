extern crate alloc;

use crate::boot_info::BootInfo;
#[cfg(test)]
use crate::debug;
use crate::efi::fs::EfiFileName;
use crate::error;
use crate::error::Error;
use crate::error::Result;
use crate::executor::yield_execution;
use crate::info;
use crate::loader::Elf;
use crate::net::dns::query_dns;
use crate::net::dns::DnsResponseEntry;
use crate::net::icmp::IcmpPacket;
use crate::net::manager::Network;
use crate::net::tcp::TcpSocket;
use crate::println;
use crate::x86_64::trigger_debug_interrupt;
use alloc::vec::Vec;
use core::str::FromStr;
use noli::mem::Sliceable;
use noli::net::IpV4Addr;

async fn run_app(name: &str, args: &[&str]) -> Result<i64> {
    let boot_info = BootInfo::take();
    let root_files = boot_info.root_files();
    let root_files: alloc::vec::Vec<&crate::boot_info::File> =
        root_files.iter().filter_map(|e| e.as_ref()).collect();
    let name = EfiFileName::from_str(name)?;
    let elf = root_files.iter().find(|&e| e.name() == &name);
    if let Some(elf) = elf {
        let elf = Elf::parse(elf)?;
        let app = elf.load()?;
        let result = app.exec(args).await?;
        #[cfg(test)]
        if result == 0 {
            debug::exit_qemu(debug::QemuExitCode::Success);
        } else {
            debug::exit_qemu(debug::QemuExitCode::Fail);
        }
        #[cfg(not(test))]
        Ok(result)
    } else {
        Err(Error::Failed("command::run_app: No such file or app"))
    }
}

pub async fn run(cmdline: &str) -> Result<()> {
    let network = Network::take();
    let args = cmdline.trim();
    let args: Vec<&str> = args.split(' ').collect();
    println!("\n{args:?}");
    if let Some(&cmd) = args.first() {
        match cmd {
            "panic" => {
                trigger_debug_interrupt();
            }
            "wait_until_network_is_up" => {
                while network.router().is_none() {
                    yield_execution().await;
                }
            }
            "ip" => {
                println!("netmask: {:?}", network.netmask());
                println!("router: {:?}", network.router());
                println!("dns: {:?}", network.dns());
            }
            "ping" => {
                if let Some(ip) = args.get(1) {
                    let ip = IpV4Addr::from_str(ip);
                    if let Ok(ip) = ip {
                        network.send_ip_packet(IcmpPacket::new_request(ip).copy_into_slice());
                    } else {
                        println!("{ip:?}")
                    }
                } else {
                    println!("usage: ip <target_ipv4_addr>")
                }
            }
            "wait_until_dns_ready" => loop {
                if let Some(dns_ip) = network.dns() {
                    if let Some(eth) = network.arp_table_get(dns_ip) {
                        info!("DNS server eth: {eth}");
                        break;
                    } else {
                        yield_execution().await;
                    }
                } else {
                    yield_execution().await;
                }
            },
            "httpget" => {
                let host = if let Some(host) = args.get(1) {
                    host
                } else {
                    "10.0.2.2"
                };
                let port = if let Some(port) = args.get(2) {
                    port
                } else {
                    "15000"
                };
                let port = if let Ok(port) = u16::from_str(port) {
                    port
                } else {
                    return Err(Error::Failed("Failed to parse the port number"));
                };
                let ip = if let Ok(ip) = IpV4Addr::from_str(&host) {
                    ip
                } else if let Some(DnsResponseEntry::A { addr, name: _ }) =
                    query_dns(&host).await?.get(0)
                {
                    *addr
                } else {
                    return Ok(());
                };
                let sock = TcpSocket::open(ip, port);
                println!("{sock:?}")
            }
            "arp" => {
                println!("{:?}", network.arp_table_cloned())
            }
            "nslookup" => {
                if let Some(query) = args.get(1) {
                    let res = query_dns(query).await?;
                    println!("{res:?}");
                    info!("nslookup done");
                } else {
                    println!("usage: nslookup <query>")
                }
            }
            app_name => {
                let result = run_app(app_name, &args).await;
                if result.is_ok() {
                    info!("{result:?}");
                } else {
                    error!("{result:?}");
                }
            }
        }
    }
    Ok(())
}
