extern crate alloc;

use crate::boot_info::BootInfo;
#[cfg(test)]
use crate::debug_exit;
use crate::efi::fs::EfiFileName;
use crate::elf::Elf;
use crate::error::Error;
use crate::error::Result;
use crate::net::icmp::IcmpPacket;
use crate::net::ip::IpV4Addr;
use crate::net::Network;
use crate::println;
use crate::util::Sliceable;
use alloc::vec::Vec;
use core::arch::asm;
use core::str::FromStr;

fn run_app(name: &str) -> Result<i64> {
    let boot_info = BootInfo::take();
    let root_files = boot_info.root_files();
    let root_files: alloc::vec::Vec<&crate::boot_info::File> =
        root_files.iter().filter_map(|e| e.as_ref()).collect();
    let name = EfiFileName::from_str(name)?;
    let elf = root_files.iter().find(|&e| e.name() == &name);
    if let Some(elf) = elf {
        let elf = Elf::parse(elf)?;
        let app = elf.load()?;
        let result = app.exec()?;
        #[cfg(test)]
        if result == 0 {
            debug_exit::exit_qemu(debug_exit::QemuExitCode::Success);
        } else {
            debug_exit::exit_qemu(debug_exit::QemuExitCode::Fail);
        }
        #[cfg(not(test))]
        Ok(result)
    } else {
        Err(Error::Failed("Init app file not found"))
    }
}

pub fn run(cmdline: &str) -> Result<()> {
    let network = Network::take();
    let args = cmdline.trim();
    let args: Vec<&str> = args.split(' ').collect();
    println!("\n{args:?}");
    if let Some(&cmd) = args.first() {
        match cmd {
            "panic" => unsafe {
                asm!("int3");
            },
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
            "arp" => {
                println!("{:?}", network.arp_table_cloned())
            }
            app_name => {
                let result = run_app(app_name);
                println!("{result:?}");
            }
        }
    }
    Ok(())
}
