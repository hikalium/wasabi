extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::executor::TimeoutFuture;
use crate::mutex::Mutex;
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
use core::mem::MaybeUninit;
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
struct IpV4Protocol(u8);
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
#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
struct IpV4Packet {
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

// https://datatracker.ietf.org/doc/html/rfc2131
// 4.1 Constructing and sending DHCP messages
const UDP_PORT_DHCP_SERVER: u16 = 67;
const UDP_PORT_DHCP_CLIENT: u16 = 68;

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
struct UdpPacket {
    ip: IpV4Packet,
    src_port: [u8; 2], // optional
    dst_port: [u8; 2],
    data_size: [u8; 2],
    csum: InternetChecksum,
}
impl UdpPacket {
    pub fn src_port(&self) -> u16 {
        u16::from_be_bytes(self.src_port)
    }
    pub fn dst_port(&self) -> u16 {
        u16::from_be_bytes(self.dst_port)
    }
    fn set_src_port(&mut self, port: u16) {
        self.src_port = port.to_be_bytes();
    }
    fn set_dst_port(&mut self, port: u16) {
        self.dst_port = port.to_be_bytes();
    }
    fn set_data_size(&mut self, data_size: u16) {
        self.data_size = data_size.to_be_bytes();
    }
}
unsafe impl Sliceable for UdpPacket {}

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
struct InternetChecksum([u8; 2]);
impl InternetChecksum {
    pub fn calc(data: &[u8]) -> Self {
        // https://tools.ietf.org/html/rfc1071
        InternetChecksumGenerator::new().feed(data).checksum()
    }
}

// https://tools.ietf.org/html/rfc1071
#[derive(Copy, Clone, Default)]
struct InternetChecksumGenerator {
    sum: u32,
}
impl InternetChecksumGenerator {
    fn new() -> Self {
        Self::default()
    }
    fn feed(mut self, data: &[u8]) -> Self {
        let iter = data.chunks(2);
        for w in iter {
            self.sum += ((w[0] as u32) << 8) | w.get(1).cloned().unwrap_or_default() as u32;
        }
        self
    }
    fn checksum(mut self) -> InternetChecksum {
        while (self.sum >> 16) != 0 {
            self.sum = (self.sum & 0xffff) + (self.sum >> 16);
        }
        InternetChecksum((!self.sum as u16).to_be_bytes())
    }
}

#[test_case]
fn internet_checksum() {
    // https://datatracker.ietf.org/doc/html/rfc1071
    assert_eq!(
        InternetChecksumGenerator::new().checksum(),
        InternetChecksum([0xff, 0xff])
    );
    assert_eq!(
        InternetChecksumGenerator::new()
            .feed(&[
                0x00, 0x45, 0x73, 0x00, 0x00, 0x00, 0x00, 0x40, 0x11, 0x40, 0x00, 0x00, 0xa8, 0xc0,
                0x01, 0x00, 0xa8, 0xc0, 0xc7, 0x00
            ])
            .checksum(),
        InternetChecksum([0x61, 0xb8])
    );
    assert_eq!(
        InternetChecksumGenerator::new()
            .feed(&[0x00, 0x45, 0x73, 0x00, 0x00, 0x00])
            .feed(&[0x00, 0x40, 0x11, 0x40, 0x00, 0x00, 0xa8, 0xc0])
            .feed(&[0x01, 0x00, 0xa8, 0xc0, 0xc7, 0x00])
            .checksum(),
        InternetChecksum([0x61, 0xb8])
    );
}

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone)]
struct DhcpPacket {
    udp: UdpPacket,
    op: u8,
    htype: u8,
    hlen: u8,
    hops: u8,
    xid: u32,
    secs: u16,
    flags: u16,
    ciaddr: IpV4Addr,
    yiaddr: IpV4Addr,
    siaddr: IpV4Addr,
    giaddr: IpV4Addr,
    chaddr: EthernetAddr,
    chaddr_padding: [u8; 10],
    sname: [u8; 64],
    file: [u8; 128],
    cookie: [u8; 4],
    // Optional fields follow
}
const _: () = assert!(size_of::<DhcpPacket>() == 282);
impl DhcpPacket {
    pub fn request(src_eth_addr: EthernetAddr) -> Self {
        let mut this = Self::default();
        // eth
        this.udp.ip.eth.dst = EthernetAddr::broardcast();
        this.udp.ip.eth.src = src_eth_addr;
        this.udp.ip.eth.eth_type = EthernetType::ip_v4();
        // ip
        this.udp.ip.version_and_ihl = 0x45; // IPv4, header len = 5 * sizeof(uint32_t) = 20 bytes
        this.udp
            .ip
            .set_data_length((size_of::<Self>() - size_of::<IpV4Packet>()) as u16);
        this.udp.ip.ident = 0x426b;
        this.udp.ip.ttl = 0xff;
        this.udp.ip.protocol = IpV4Protocol::udp();
        this.udp.ip.dst = IpV4Addr::broardcast();
        this.udp.ip.calc_checksum();
        // udp
        this.udp.set_src_port(UDP_PORT_DHCP_CLIENT);
        this.udp.set_dst_port(UDP_PORT_DHCP_SERVER);
        this.udp
            .set_data_size((size_of::<Self>() - size_of::<UdpPacket>()) as u16);
        // dhcp
        this.op = 1;
        this.htype = 1;
        this.hlen = 6;
        this.xid = 0x1234;
        this.chaddr = src_eth_addr;
        // https://tools.ietf.org/html/rfc2131
        // 3. The Client-Server Protocol
        this.cookie = [99, 130, 83, 99];
        // this.udp.csum can be 0 since it is optional
        /*
        this.udp.csum = InternetChecksumGenerator::new()
            .feed(&this.as_slice()[size_of::<IpV4Packet>()..])
            .feed(UdpFakeIpHeader::new(&this.udp.ip, this.udp.data_size).as_slice())
            .checksum();
        */
        this
    }
}
impl Default for DhcpPacket {
    fn default() -> Self {
        // SAFETY: This is safe since DhcpPacket is valid as a data for any contents
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}
unsafe impl Sliceable for DhcpPacket {}

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
}
impl Network {
    fn new() -> Self {
        Self {
            interfaces: Mutex::new(Vec::new(), "Network"),
            interface_has_added: AtomicBool::new(false),
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
}
static NETWORK: Mutex<Option<Rc<Network>>> = Mutex::new(None, "NETWORK");

fn handle_receive_udp(packet: &[u8]) -> Result<()> {
    let udp = UdpPacket::from_slice(packet)?;
    match (udp.src_port(), udp.dst_port()) {
        (UDP_PORT_DHCP_SERVER, UDP_PORT_DHCP_CLIENT) => {
            let dhcp = DhcpPacket::from_slice(packet)?;
            println!("DHCP SERVER -> CLIENT yiaddr = {}", dhcp.yiaddr);
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
