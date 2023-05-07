extern crate alloc;

mod checksum;
mod dhcp;
mod udp;

use crate::error::Error;
use crate::error::Result;
use crate::executor::TimeoutFuture;
use crate::mutex::Mutex;
use crate::net::checksum::InternetChecksum;
use crate::net::dhcp::DhcpPacket;
use crate::net::dhcp::DHCP_OPT_DNS;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_ACK;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_DISCOVER;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_OFFER;
use crate::net::dhcp::DHCP_OPT_NETMASK;
use crate::net::dhcp::DHCP_OPT_ROUTER;
use crate::net::dhcp::DHCP_OP_BOOTREPLY;
use crate::net::udp::UdpPacket;
use crate::net::udp::UDP_PORT_DHCP_CLIENT;
use crate::net::udp::UDP_PORT_DHCP_SERVER;
use crate::println;
use crate::util::Sliceable;
use alloc::boxed::Box;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use alloc::rc::Rc;
use alloc::rc::Weak;
use alloc::vec::Vec;
use core::mem::size_of;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct EthernetType {
    value: [u8; 2],
}
impl EthernetType {
    const fn ip_v4() -> Self {
        Self {
            value: [0x08, 0x00],
        }
    }
    const fn arp() -> Self {
        Self {
            value: [0x08, 0x06],
        }
    }
}
#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct EthernetAddr {
    mac: [u8; 6],
}
impl EthernetAddr {
    pub fn new(mac: [u8; 6]) -> Self {
        Self { mac }
    }
    const fn broardcast() -> Self {
        Self {
            mac: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        }
    }
    const fn zero() -> Self {
        Self {
            mac: [0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        }
    }
}
impl Debug for EthernetAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5],
        )
    }
}
#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct EthernetHeader {
    dst: EthernetAddr,
    src: EthernetAddr,
    eth_type: EthernetType,
}
const _: () = assert!(size_of::<EthernetHeader>() == 14);
impl EthernetHeader {
    pub fn eth_type(&self) -> EthernetType {
        self.eth_type
    }
}
unsafe impl Sliceable for EthernetHeader {}

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct ArpPacket {
    eth_header: EthernetHeader,
    hw_type: [u8; 2],
    proto_type: [u8; 2],
    hw_addr_size: u8,
    proto_addr_size: u8,
    op: [u8; 2],
    sender_mac: EthernetAddr,
    sender_ip: IpV4Addr,
    target_mac: EthernetAddr,
    target_ip: IpV4Addr,
}
const _: () = assert!(size_of::<ArpPacket>() == 42);
impl ArpPacket {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // SAFETY: any bytes can be parsed as the ARP packet
        if bytes.len() >= size_of::<ArpPacket>() {
            let mut tmp = [0u8; size_of::<ArpPacket>()];
            tmp.copy_from_slice(&bytes[0..size_of::<ArpPacket>()]);
            Ok(unsafe { core::mem::transmute(tmp) })
        } else {
            Err(Error::Failed("too short"))
        }
    }
    pub fn request(src_eth: EthernetAddr, src_ip: IpV4Addr, dst_ip: IpV4Addr) -> Self {
        Self {
            eth_header: EthernetHeader {
                dst: src_eth,
                src: EthernetAddr::broardcast(),
                eth_type: EthernetType::arp(),
            },
            hw_type: [0x00, 0x01],
            proto_type: [0x08, 0x00],
            hw_addr_size: 6,
            proto_addr_size: 4,
            op: [0x00, 0x01],
            sender_mac: src_eth,
            sender_ip: src_ip,
            target_mac: EthernetAddr::zero(), // target_mac = Unknown
            target_ip: dst_ip,
        }
    }
}
impl Debug for ArpPacket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ArpPacket {{ op: {}, sender: {:?} ({:?}), target: {:?} ({:?}) }}",
            match (self.op[0], self.op[1]) {
                (0, 1) => "request",
                (0, 2) => "response",
                (_, _) => "unknown",
            },
            self.sender_ip,
            self.sender_mac,
            self.target_ip,
            self.target_mac,
        )
    }
}
unsafe impl Sliceable for ArpPacket {}

#[repr(transparent)]
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct IpV4Protocol(u8);
impl IpV4Protocol {
    /*
    pub fn icmp() -> Self {
        Self(1)
    }
    pub fn tcp() -> Self {
        Self(6)
    }
    */
    pub const fn udp() -> Self {
        Self(17)
    }
}
#[repr(transparent)]
#[allow(unused)]
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct IpV4Addr([u8; 4]);
impl IpV4Addr {
    pub fn new(ip: [u8; 4]) -> Self {
        Self(ip)
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
    const fn broardcast() -> Self {
        Self([0xff, 0xff, 0xff, 0xff])
    }
}
unsafe impl Sliceable for IpV4Addr {}
#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct IpV4Packet {
    eth: EthernetHeader,
    version_and_ihl: u8,
    dscp_and_ecn: u8,
    length: [u8; 2],
    ident: u16,
    flags: u16,
    ttl: u8,
    protocol: IpV4Protocol,
    csum: InternetChecksum,
    src: IpV4Addr,
    dst: IpV4Addr,
}
impl IpV4Packet {
    pub fn protocol(&self) -> IpV4Protocol {
        self.protocol
    }
    fn set_data_length(&mut self, mut size: u16) {
        size += (size_of::<Self>() - size_of::<EthernetHeader>()) as u16; // IP header size
        size = (size + 1) & !1; // make size odd
        self.length = size.to_be_bytes()
    }
    fn calc_checksum(&mut self) {
        self.csum = InternetChecksum::calc(&self.as_slice()[size_of::<EthernetHeader>()..]);
    }
}
unsafe impl Sliceable for IpV4Packet {}

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
