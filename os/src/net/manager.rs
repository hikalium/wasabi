extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::executor::Task;
use crate::executor::TimeoutFuture;
use crate::executor::ROOT_EXECUTOR;
use crate::info;
use crate::mutex::Mutex;
use crate::net::arp::ArpPacket;
use crate::net::checksum::InternetChecksum;
use crate::net::dhcp::DhcpPacket;
use crate::net::dhcp::DHCP_OPT_DNS;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_ACK;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_DISCOVER;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_END;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_OFFER;
use crate::net::dhcp::DHCP_OPT_MESSAGE_TYPE_PADDING;
use crate::net::dhcp::DHCP_OPT_NETMASK;
use crate::net::dhcp::DHCP_OPT_ROUTER;
use crate::net::dns::parse_dns_response;
use crate::net::dns::PORT_DNS_SERVER;
use crate::net::eth::EthernetAddr;
use crate::net::eth::EthernetHeader;
use crate::net::eth::EthernetType;
use crate::net::icmp::IcmpPacket;
use crate::net::ip::IpV4Packet;
use crate::net::ip::IpV4Protocol;
use crate::net::tcp::TcpPacket;
use crate::net::tcp::TcpSocket;
use crate::net::tcp::TcpSocketState;
use crate::net::udp::UdpPacket;
use crate::net::udp::UdpSocket;
use crate::net::udp::UDP_PORT_DHCP_CLIENT;
use crate::net::udp::UDP_PORT_DHCP_SERVER;
use crate::warn;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::rc::Weak;
use alloc::vec::Vec;
use core::mem::size_of;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use noli::mem::Sliceable;
use noli::net::IpV4Addr;

pub trait NetworkInterface {
    fn name(&self) -> &str;
    fn ethernet_addr(&self) -> EthernetAddr;
    fn push_packet(&self, packet: Box<[u8]>) -> Result<()>;
    fn pop_packet(&self) -> Result<Box<[u8]>> {
        Err(Error::Failed("Not implemented yet"))
    }
}

pub type ArpTable = BTreeMap<IpV4Addr, (EthernetAddr, Weak<dyn NetworkInterface>)>;
pub type TcpSocketTable = BTreeMap<u16, Rc<TcpSocket>>;
pub type UdpSocketTable = BTreeMap<u16, Rc<UdpSocket>>;

pub struct Network {
    interfaces: Mutex<Vec<Weak<dyn NetworkInterface>>>,
    interface_has_added: AtomicBool,
    netmask: Mutex<Option<IpV4Addr>>,
    router: Mutex<Option<IpV4Addr>>,
    dns: Mutex<Option<IpV4Addr>>,
    self_ip: Mutex<Option<IpV4Addr>>,
    ip_tx_queue: Mutex<VecDeque<Box<[u8]>>>,
    tcp_socket_table: Mutex<TcpSocketTable>,
    udp_socket_table: Mutex<UdpSocketTable>,
    arp_table: Mutex<ArpTable>,
}
impl Network {
    fn new() -> Self {
        Self {
            interfaces: Mutex::new(Vec::new()),
            interface_has_added: AtomicBool::new(false),
            netmask: Mutex::new(None),
            router: Mutex::new(None),
            dns: Mutex::new(None),
            self_ip: Mutex::new(None),
            ip_tx_queue: Mutex::new(VecDeque::new()),
            tcp_socket_table: Mutex::new(BTreeMap::new()),
            udp_socket_table: Mutex::new(BTreeMap::new()),
            arp_table: Mutex::new(BTreeMap::new()),
        }
    }
    pub fn take() -> Rc<Network> {
        let mut network = NETWORK.lock();
        let network = network.get_or_insert_with(|| {
            let network = Self::new();
            network.register_tcp_socket(18080, Rc::new(TcpSocket::new(TcpSocketState::Listen)));
            let dns_client = Rc::new(UdpSocket::default());
            network.register_udp_socket(PORT_DNS_SERVER, dns_client.clone());
            {
                let task = Task::new(async { network_manager_thread().await });
                ROOT_EXECUTOR.lock().spawn(task);
            }
            {
                let task = Task::new(async move {
                    loop {
                        let dns_packet = dns_client.recv().await;
                        parse_dns_response(&dns_packet)?;
                    }
                });
                ROOT_EXECUTOR.lock().spawn(task);
            }
            Rc::new(network)
        });
        network.clone()
    }
    pub fn register_interface(&self, iface: Weak<dyn NetworkInterface>) {
        let mut interfaces = self.interfaces.lock();
        interfaces.push(iface);
        self.interface_has_added.store(true, Ordering::SeqCst);
    }
    pub fn register_tcp_socket(&self, port: u16, s: Rc<TcpSocket>) {
        self.tcp_socket_table.lock().insert(port, s);
    }
    pub fn register_udp_socket(&self, port: u16, s: Rc<UdpSocket>) {
        self.udp_socket_table.lock().insert(port, s);
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
    pub fn set_self_ip(&self, value: Option<IpV4Addr>) {
        *self.self_ip.lock() = value;
    }
    pub fn send_ip_packet(&self, packet: Box<[u8]>) {
        self.ip_tx_queue.lock().push_back(packet)
    }
    pub fn arp_table_cloned(&self) -> ArpTable {
        self.arp_table.lock().clone()
    }
    pub fn arp_table_register(
        &self,
        ip_addr: IpV4Addr,
        eth_addr: EthernetAddr,
        iface: Weak<dyn NetworkInterface>,
    ) {
        self.arp_table.lock().insert(ip_addr, (eth_addr, iface));
    }
    pub fn arp_table_get(&self, ip_addr: IpV4Addr) -> Option<EthernetAddr> {
        self.arp_table.lock().get(&ip_addr).map(|e| e.0)
    }
}
static NETWORK: Mutex<Option<Rc<Network>>> = Mutex::new(None);

fn handle_rx_dhcp_client(packet: &[u8], iface: &Rc<dyn NetworkInterface>) -> Result<()> {
    let network = Network::take();
    // TODO(hikalium): impl check for xid and cookie
    let dhcp = DhcpPacket::from_slice(packet)?;
    if !dhcp.is_boot_reply() {
        return Ok(());
    }
    info!(
        "net: rx: DHCP: SERVER -> CLIENT yiaddr = {} chaddr = {}",
        dhcp.yiaddr(),
        dhcp.chaddr()
    );
    let new_self_ip = dhcp.yiaddr();
    network.set_self_ip(Some(new_self_ip));
    let options = &packet[size_of::<DhcpPacket>()..];
    let mut it = options.iter();
    while let Some(op) = it.next().cloned() {
        if op == DHCP_OPT_MESSAGE_TYPE_PADDING {
            continue;
        }
        if op == DHCP_OPT_MESSAGE_TYPE_END {
            break;
        }
        if let Some(len) = it.next().cloned() {
            if len == 0 {
                break;
            }
            let data: Vec<u8> = it.clone().take(len as usize).cloned().collect();
            match op {
                DHCP_OPT_MESSAGE_TYPE => match data[0] {
                    DHCP_OPT_MESSAGE_TYPE_ACK => {
                        info!("DHCPACK");
                    }
                    DHCP_OPT_MESSAGE_TYPE_OFFER => {
                        info!("DHCPOFFER");
                    }
                    DHCP_OPT_MESSAGE_TYPE_DISCOVER => {
                        info!("DHCPDISCOVER");
                    }
                    t => {
                        info!("DHCP MESSAGE_TYPE = {t}");
                    }
                },
                DHCP_OPT_NETMASK => {
                    if let Ok(netmask) = IpV4Addr::from_slice(&data) {
                        info!("netmask: {netmask}");
                        network.set_netmask(Some(*netmask));
                    }
                }
                DHCP_OPT_ROUTER => {
                    if let Ok(router) = IpV4Addr::from_slice(&data) {
                        info!("router: {router}");
                        network.set_router(Some(*router));
                        let arp_req =
                            ArpPacket::request(iface.ethernet_addr(), new_self_ip, *router);
                        iface.push_packet(arp_req.copy_into_slice())?;
                    }
                }
                DHCP_OPT_DNS => {
                    if let Ok(dns) = IpV4Addr::from_slice(&data) {
                        info!("dns: {dns}");
                        network.set_dns(Some(*dns));
                        let arp_req = ArpPacket::request(iface.ethernet_addr(), new_self_ip, *dns);
                        iface.push_packet(arp_req.copy_into_slice())?;
                    }
                }
                _ => {}
            }
            it.advance_by(len as usize)
                .or(Err(Error::Failed("Invalid op data len")))?;
        }
    }
    Ok(())
}

fn handle_rx_udp(packet: &[u8], iface: &Rc<dyn NetworkInterface>) -> Result<()> {
    let udp = UdpPacket::from_slice(packet)?;
    match (udp.src_port(), udp.dst_port()) {
        (UDP_PORT_DHCP_SERVER, UDP_PORT_DHCP_CLIENT) => handle_rx_dhcp_client(packet, iface),
        (_, dst) => {
            if let Some(sock) = Network::take().udp_socket_table.lock().get(&dst) {
                sock.handle_rx(packet)
            } else {
                info!("net: rx: in (no listening socket) : {udp:?}",);
                Ok(())
            }
        }
    }
}

fn handle_rx_tcp(in_bytes: &[u8]) -> Result<()> {
    let in_packet = Vec::from(in_bytes);
    let in_tcp = TcpPacket::from_slice(&in_packet)?;
    if let Some(sock) = Network::take()
        .tcp_socket_table
        .lock()
        .get(&in_tcp.dst_port())
    {
        sock.handle_rx(in_bytes)?;
    } else {
        info!("net: rx: in (no listening socket) : {in_tcp:?}",);
    }
    Ok(())
}

fn handle_rx_icmp(packet: &[u8]) -> Result<()> {
    let icmp = IcmpPacket::from_slice(packet)?;
    info!("net: rx: ICMP: {icmp:?}");
    Ok(())
}
fn handle_rx_arp(packet: &[u8], iface: &Rc<dyn NetworkInterface>) -> Result<()> {
    if let Ok(arp) = ArpPacket::from_slice(packet) {
        if arp.is_response() {
            Network::take().arp_table_register(
                arp.sender_ip_addr(),
                arp.sender_eth_addr(),
                Rc::downgrade(iface),
            )
        }
        Ok(())
    } else {
        Err(Error::Failed("handle_rx_arp: Not a valid ARP Packet"))
    }
}

fn handle_receive(packet: &[u8], iface: &Rc<dyn NetworkInterface>) -> Result<()> {
    match EthernetHeader::from_slice(packet)?.eth_type() {
        e if e == EthernetType::ip_v4() => match IpV4Packet::from_slice(packet)?.protocol() {
            e if e == IpV4Protocol::udp() => handle_rx_udp(packet, iface),
            e if e == IpV4Protocol::tcp() => handle_rx_tcp(packet),
            e if e == IpV4Protocol::icmp() => handle_rx_icmp(packet),
            e => {
                warn!("handle_receive: Unknown ip_v4.protocol: {e:?}");
                Ok(())
            }
        },
        e if e == EthernetType::arp() => handle_rx_arp(packet, iface),
        e => {
            warn!("handle_receive: Unknown eth_type {e:?}");
            Ok(())
        }
    }
}

fn probe_interfaces() -> Result<()> {
    let network = Network::take();
    let interfaces = network.interfaces.lock();
    if network
        .interface_has_added
        .compare_exchange_weak(true, false, Ordering::SeqCst, Ordering::Relaxed)
        .is_ok()
    {
        info!("Network: network interfaces updated:");
        for iface in &*interfaces {
            if let Some(iface) = iface.upgrade() {
                info!("  {:?} {}", iface.ethernet_addr(), iface.name());
                let dhcp_req = DhcpPacket::request(iface.ethernet_addr())?;
                iface.push_packet(dhcp_req.copy_into_slice())?;
            }
        }
    }
    Ok(())
}

fn process_tx() -> Result<()> {
    let network = Network::take();
    if let Some(mut org_packet) = network.ip_tx_queue.lock().pop_front() {
        if let Ok(ip_packet) = IpV4Packet::from_slice_mut(&mut org_packet) {
            let dst_ip = ip_packet.dst();
            if let (Some(src_ip), Some(mask)) = (*network.self_ip.lock(), *network.netmask.lock()) {
                let network_prefix = src_ip.network_prefix(mask);
                let next_hop_info = if network_prefix == dst_ip.network_prefix(mask) {
                    network.arp_table.lock().get(&dst_ip).cloned()
                } else {
                    network
                        .router
                        .lock()
                        .and_then(|router_ip| network.arp_table.lock().get(&router_ip).cloned())
                };
                if let Some((next_hop, iface)) = next_hop_info {
                    ip_packet.set_src(src_ip);
                    if let Some(iface) = iface.upgrade() {
                        ip_packet.eth = EthernetHeader::new(
                            next_hop,
                            iface.ethernet_addr(),
                            EthernetType::ip_v4(),
                        );
                        ip_packet.clear_checksum();
                        let csum = InternetChecksum::calc(
                            &org_packet[size_of::<EthernetHeader>()..size_of::<IpV4Packet>()],
                        );
                        if let Ok(ip_packet) = IpV4Packet::from_slice_mut(&mut org_packet) {
                            ip_packet.set_checksum(csum);
                            iface.push_packet(org_packet.clone())?;
                        }
                    }
                } else {
                    warn!("No route to {dst_ip}. Sending ARP from all the interfaces.");
                    let interfaces = network.interfaces.lock();
                    for iface in &*interfaces {
                        if let Some(iface) = iface.upgrade() {
                            let arp_req = ArpPacket::request(iface.ethernet_addr(), src_ip, dst_ip);
                            iface.push_packet(arp_req.copy_into_slice())?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
fn process_rx() -> Result<()> {
    let network = Network::take();
    let interfaces = network.interfaces.lock();
    for iface in &*interfaces {
        if let Some(iface) = iface.upgrade() {
            if let Ok(packet) = iface.pop_packet() {
                handle_receive(&packet, &iface)?;
            }
        }
    }
    Ok(())
}

pub async fn network_manager_thread() -> Result<()> {
    info!("Network manager started running");
    loop {
        probe_interfaces()?;
        process_tx()?;
        process_rx()?;
        TimeoutFuture::new_ms(100).await;
    }
}
