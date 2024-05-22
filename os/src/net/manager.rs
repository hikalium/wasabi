extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::executor::TimeoutFuture;
use crate::info;
use crate::mutex::Mutex;
use crate::net::arp::ArpPacket;
use crate::net::checksum::InternetChecksum;
use crate::net::checksum::InternetChecksumGenerator;
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
use crate::net::eth::EthernetAddr;
use crate::net::eth::EthernetHeader;
use crate::net::eth::EthernetType;
use crate::net::icmp::IcmpPacket;
use crate::net::ip::IpV4Addr;
use crate::net::ip::IpV4Packet;
use crate::net::ip::IpV4Protocol;
use crate::net::tcp::TcpPacket;
use crate::net::udp::UdpPacket;
use crate::net::udp::UDP_PORT_DHCP_CLIENT;
use crate::net::udp::UDP_PORT_DHCP_SERVER;
use crate::util::Sliceable;
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

pub trait NetworkInterface {
    fn name(&self) -> &str;
    fn ethernet_addr(&self) -> EthernetAddr;
    fn push_packet(&self, packet: Box<[u8]>) -> Result<()>;
    fn pop_packet(&self) -> Result<Box<[u8]>> {
        Err(Error::Failed("Not implemented yet"))
    }
}

pub type ArpTable = BTreeMap<IpV4Addr, (EthernetAddr, Weak<dyn NetworkInterface>)>;
pub struct Network {
    interfaces: Mutex<Vec<Weak<dyn NetworkInterface>>>,
    interface_has_added: AtomicBool,
    netmask: Mutex<Option<IpV4Addr>>,
    router: Mutex<Option<IpV4Addr>>,
    dns: Mutex<Option<IpV4Addr>>,
    self_ip: Mutex<Option<IpV4Addr>>,
    ip_tx_queue: Mutex<VecDeque<Box<[u8]>>>,
    arp_table: Mutex<ArpTable>,
}
impl Network {
    fn new() -> Self {
        Self {
            interfaces: Mutex::new(Vec::new(), "Network.interfaces"),
            interface_has_added: AtomicBool::new(false),
            netmask: Mutex::new(None, "Network.netmask"),
            router: Mutex::new(None, "Network.router"),
            dns: Mutex::new(None, "Network.dns"),
            self_ip: Mutex::new(None, "Network.self_ip"),
            ip_tx_queue: Mutex::new(VecDeque::new(), "Network.ip_tx_queue"),
            arp_table: Mutex::new(BTreeMap::new(), "Network.arp_table"),
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
}
static NETWORK: Mutex<Option<Rc<Network>>> = Mutex::new(None, "NETWORK");

fn handle_rx_dhcp_client(packet: &[u8]) -> Result<()> {
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
    network.set_self_ip(Some(dhcp.yiaddr()));
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
            info!("op = {op}, data = {data:?}");
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
                    }
                }
                DHCP_OPT_DNS => {
                    if let Ok(dns) = IpV4Addr::from_slice(&data) {
                        info!("dns: {dns}");
                        network.set_dns(Some(*dns));
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

fn handle_rx_udp(packet: &[u8]) -> Result<()> {
    let udp = UdpPacket::from_slice(packet)?;
    match (udp.src_port(), udp.dst_port()) {
        (UDP_PORT_DHCP_SERVER, UDP_PORT_DHCP_CLIENT) => handle_rx_dhcp_client(packet),
        (src, dst) => {
            info!("net: rx: UDP :{src} -> :{dst}");
            Ok(())
        }
    }
}

fn handle_rx_tcp(in_bytes: &[u8]) -> Result<()> {
    let in_packet = Vec::from(in_bytes);
    let in_tcp = TcpPacket::from_slice(&in_packet)?;
    info!("net: rx: in : {in_tcp:?}",);
    let data = &in_packet[(size_of::<IpV4Packet>() + in_tcp.header_len())..]
        [..(in_tcp.ip.payload_size() - in_tcp.header_len())];
    if !data.is_empty() {
        info!("net: rx: TCP: data: {data:X?}");
    }
    {
        let mut out_bytes = Vec::new();
        {
            let from_ip = in_tcp.ip.dst();
            let to_ip = in_tcp.ip.src();
            let from_port = in_tcp.dst_port();
            let to_port = in_tcp.src_port();

            out_bytes.resize(size_of::<TcpPacket>(), 0);
            let eth = EthernetHeader::new(
                EthernetAddr::zero(),
                EthernetAddr::zero(),
                EthernetType::ip_v4(),
            );
            let ip_data_length = out_bytes.len() - size_of::<IpV4Packet>();
            let ipv4_packet =
                IpV4Packet::new(eth, to_ip, from_ip, IpV4Protocol::tcp(), ip_data_length);
            let out_tcp = TcpPacket::from_slice_mut(&mut out_bytes)?;
            out_tcp.set_header_len_nibble(5);
            out_tcp.ip = ipv4_packet;

            out_tcp.ip.set_src(from_ip);
            out_tcp.ip.set_dst(to_ip);

            out_tcp.set_src_port(from_port);
            out_tcp.set_dst_port(to_port);

            out_tcp.set_seq_num(12345);
            out_tcp.set_syn();
            out_tcp.set_ack();
            out_tcp.set_window(0xffff);
            out_tcp.set_ack_num(in_tcp.seq_num().wrapping_add(1));

            out_tcp.csum = InternetChecksum::default();
        }
        let mut csum = InternetChecksumGenerator::new();
        csum.feed(&out_bytes[size_of::<IpV4Packet>()..]);
        {
            let out_tcp = TcpPacket::from_slice_mut(&mut out_bytes)?;
            csum.feed(out_tcp.ip.src().as_slice());
            csum.feed(out_tcp.ip.dst().as_slice());
            csum.feed(&[0x00, out_tcp.ip.protocol().0]);
            csum.feed(&20u16.to_be_bytes()); // TCP Header + TCP Data size
            out_tcp.csum = csum.checksum();
            info!("net: rx: out: {out_tcp:?}",);
            crate::print::hexdump(out_tcp.as_slice());
        }
        Network::take().send_ip_packet(out_bytes.into_boxed_slice());
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
        info!("net: rx: ARP: {arp:?}");
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
            e if e == IpV4Protocol::udp() => handle_rx_udp(packet),
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
                    warn!("No route to {dst_ip}. Dropping the packet.");
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
