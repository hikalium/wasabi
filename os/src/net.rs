extern crate alloc;

pub mod arp;
pub mod checksum;
pub mod dhcp;
pub mod eth;
pub mod ip;
pub mod udp;

use crate::error::Error;
use crate::error::Result;
use crate::executor::TimeoutFuture;
use crate::mutex::Mutex;
use crate::net::arp::ArpPacket;
use crate::net::dhcp::DhcpPacket;
use crate::net::dhcp::DHCP_OPT_DNS;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_ACK;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_DISCOVER;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_OFFER;
use crate::net::dhcp::DHCP_OPT_NETMASK;
use crate::net::dhcp::DHCP_OPT_ROUTER;
use crate::net::dhcp::DHCP_OP_BOOTREPLY;
use crate::net::eth::EthernetAddr;
use crate::net::eth::EthernetHeader;
use crate::net::eth::EthernetType;
use crate::net::ip::IpV4Addr;
use crate::net::ip::IpV4Packet;
use crate::net::ip::IpV4Protocol;
use crate::net::udp::UdpPacket;
use crate::net::udp::UDP_PORT_DHCP_CLIENT;
use crate::net::udp::UDP_PORT_DHCP_SERVER;
use crate::println;
use crate::util::Sliceable;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::rc::Weak;
use alloc::vec::Vec;
use core::mem::size_of;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

pub trait NetworkInterface {
    fn name(&self) -> &str;
    fn ethernet_addr(&self) -> EthernetAddr;
    fn push_packet(&self, packet: Box<[u8]>) -> Result<()>;
    fn pop_packet(&self) -> Result<Box<[u8]>> {
        Err(Error::Failed("Not implemented yet"))
    }
}

pub struct Network {
    interfaces: Mutex<Vec<Weak<dyn NetworkInterface>>>,
    interface_has_added: AtomicBool,
    netmask: Mutex<Option<IpV4Addr>>,
    router: Mutex<Option<IpV4Addr>>,
    dns: Mutex<Option<IpV4Addr>>,
}
impl Network {
    fn new() -> Self {
        Self {
            interfaces: Mutex::new(Vec::new(), "Network.interfaces"),
            interface_has_added: AtomicBool::new(false),
            netmask: Mutex::new(None, "Network.netmask"),
            router: Mutex::new(None, "Network.router"),
            dns: Mutex::new(None, "Network.dns"),
        }
    }
    pub fn take() -> Rc<Network> {
        let mut network = NETWORK.lock();
        let network = network.get_or_insert_with(|| Rc::new(Self::new()));
        network.clone()
    }
    pub fn register_interface(&self, iface: Weak<dyn NetworkInterface>) {
        let mut interfaces = self.interfaces.lock();
        interfaces.push(iface);
        self.interface_has_added.store(true, Ordering::SeqCst);
    }
    pub fn netmask(&self) -> Option<IpV4Addr> {
        *self.netmask.lock()
    }
    pub fn router(&self) -> Option<IpV4Addr> {
        *self.router.lock()
    }
    pub fn dns(&self) -> Option<IpV4Addr> {
        *self.dns.lock()
    }
    pub fn set_netmask(&self, value: Option<IpV4Addr>) {
        *self.netmask.lock() = value;
    }
    pub fn set_router(&self, value: Option<IpV4Addr>) {
        *self.router.lock() = value;
    }
    pub fn set_dns(&self, value: Option<IpV4Addr>) {
        *self.dns.lock() = value;
    }
}
static NETWORK: Mutex<Option<Rc<Network>>> = Mutex::new(None, "NETWORK");

fn handle_receive_udp(packet: &[u8]) -> Result<()> {
    let udp = UdpPacket::from_slice(packet)?;
    match (udp.src_port(), udp.dst_port()) {
        (UDP_PORT_DHCP_SERVER, UDP_PORT_DHCP_CLIENT) => {
            // TODO(hikalium): impl check for xid and cookie
            let dhcp = DhcpPacket::from_slice(packet)?;
            if dhcp.op() != DHCP_OP_BOOTREPLY {
                // Not a reply, skip this message
                return Ok(());
            }
            println!("DHCP SERVER -> CLIENT yiaddr = {}", dhcp.yiaddr());
            let options = &packet[size_of::<DhcpPacket>()..];
            let mut it = options.iter();
            while let Some(op) = it.next().cloned() {
                if let Some(len) = it.next().cloned() {
                    if len == 0 {
                        break;
                    }
                    let data: Vec<u8> = it.clone().take(len as usize).cloned().collect();
                    println!("op = {op}, data = {data:?}");
                    let network = Network::take();
                    match op {
                        DHCP_OPT_MESSAGE_TYPE => match data[0] {
                            DHCP_OPT_MESSAGE_TYPE_ACK => {
                                println!("DHCPACK");
                            }
                            DHCP_OPT_MESSAGE_TYPE_OFFER => {
                                println!("DHCPOFFER");
                            }
                            DHCP_OPT_MESSAGE_TYPE_DISCOVER => {
                                println!("DHCPDISCOVER");
                            }
                            t => {
                                println!("DHCP MESSAGE_TYPE = {t}");
                            }
                        },
                        DHCP_OPT_NETMASK => {
                            if let Ok(netmask) = IpV4Addr::from_slice(&data) {
                                println!("netmask: {netmask}");
                                network.set_netmask(Some(*netmask));
                            }
                        }
                        DHCP_OPT_ROUTER => {
                            if let Ok(router) = IpV4Addr::from_slice(&data) {
                                println!("router: {router}");
                                network.set_router(Some(*router));
                            }
                        }
                        DHCP_OPT_DNS => {
                            if let Ok(dns) = IpV4Addr::from_slice(&data) {
                                println!("dns: {dns}");
                                network.set_dns(Some(*dns));
                            }
                        }
                        _ => {}
                    }
                    it.advance_by(len as usize)
                        .or(Err(Error::Failed("Invalid op data len")))?;
                }
            }
        }
        (src, dst) => {
            println!("UDP :{src} -> :{dst}");
        }
    }

    Ok(())
}

fn handle_receive(packet: &[u8]) -> Result<()> {
    if let Ok(eth) = EthernetHeader::from_slice(packet) {
        match eth.eth_type() {
            e if e == EthernetType::ip_v4() => {
                println!("IPv4");
                if let Ok(ip_v4) = IpV4Packet::from_slice(packet) {
                    match ip_v4.protocol() {
                        e if e == IpV4Protocol::udp() => {
                            println!("UDP");
                            return handle_receive_udp(packet);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub async fn network_manager_thread() -> Result<()> {
    println!("Network manager started running");
    let network = Network::take();

    loop {
        let interfaces = network.interfaces.lock();
        if network
            .interface_has_added
            .compare_exchange_weak(true, false, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            println!("Network: network interfaces updated:");
            for iface in &*interfaces {
                if let Some(iface) = iface.upgrade() {
                    println!("  {:?} {}", iface.ethernet_addr(), iface.name());
                    let arp_req = ArpPacket::request(
                        iface.ethernet_addr(),
                        IpV4Addr::new([10, 0, 2, 15]),
                        IpV4Addr::new([10, 0, 2, 2]),
                    );
                    iface.push_packet(arp_req.copy_into_slice())?;
                    let dhcp_req = DhcpPacket::request(iface.ethernet_addr());
                    iface.push_packet(dhcp_req.copy_into_slice())?;
                }
            }
        }
        for iface in &*interfaces {
            if let Some(iface) = iface.upgrade() {
                if let Ok(packet) = iface.pop_packet() {
                    handle_receive(&packet)?;
                }
            }
        }

        TimeoutFuture::new_ms(100).await;
    }
}
