extern crate alloc;

use crate::error::Result;
use crate::executor::TimeoutFuture;
use crate::mutex::Mutex;
use crate::println;
use crate::util::Sliceable;
use alloc::boxed::Box;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::rc::Rc;
use alloc::rc::Weak;
use alloc::vec::Vec;
use core::marker::PhantomPinned;
use core::mem::size_of;

#[repr(packed)]
#[derive(Clone, Copy)]
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

#[derive(Copy, Clone)]
#[repr(packed)]
pub struct IpV4Addr {
    ip: [u8; 4],
}
impl IpV4Addr {
    pub fn new(ip: [u8; 4]) -> Self {
        Self { ip }
    }
}
impl Debug for IpV4Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.ip[0], self.ip[1], self.ip[2], self.ip[3],
        )
    }
}

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone)]
pub struct EthernetHeader {
    dst_eth: EthernetAddr,
    src_eth: EthernetAddr,
    eth_type: [u8; 2],
}
const _: () = assert!(size_of::<EthernetHeader>() == 14);

#[derive(Copy, Clone)]
#[repr(packed)]
#[allow(unused)]
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
    //
    _pinned: PhantomPinned,
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
            Err(crate::error::Error::Failed("too short"))
        }
    }
    pub fn request(src_eth: EthernetAddr, src_ip: IpV4Addr, dst_ip: IpV4Addr) -> Self {
        Self {
            eth_header: EthernetHeader {
                dst_eth: src_eth,
                src_eth: EthernetAddr::broardcast(),
                eth_type: [0x08, 0x06],
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
            //
            _pinned: PhantomPinned,
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

#[allow(unused)]
mod ipv4_protocol {
    pub const ICMP: u8 = 1;
    pub const TCP: u8 = 6;
    pub const UDP: u8 = 17;
}

#[repr(packed)]
#[allow(unused)]
struct IpV4Packet {
    eth_header: EthernetHeader,
    version_and_ihl: u8,
    dscp_and_ecn: u8,
    length: [u8; 2],
    ident: u16,
    flags: u16,
    ttl: u8,
    protocol: u8,
    csum: InternetChecksum, // for this header
    src_ip: IpV4Addr,
    dst_ip: IpV4Addr,
}

#[repr(packed)]
#[allow(unused)]
struct InternetChecksum {
    // https://tools.ietf.org/html/rfc1071
    csum: [u8; 2],
}

#[repr(packed)]
#[allow(unused)]
struct UdpPacket {
    ip_header: IpV4Packet,
    src_port: [u8; 2], // optional
    dst_port: [u8; 2],
    len: [u8; 2],
    csum: InternetChecksum,
}

#[repr(packed)]
#[allow(unused)]
struct DhcpPacket {
    udp_header: UdpPacket,
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

pub trait NetworkInterface {
    fn name(&self) -> &str;
    fn ethernet_addr(&self) -> EthernetAddr;
    fn queue_packet(&self, packet: Box<[u8]>) -> Result<()>;
}

pub struct Network {
    interfaces: Mutex<Vec<Weak<dyn NetworkInterface>>>,
}
impl Network {
    fn new() -> Self {
        Self {
            interfaces: Mutex::new(Vec::new()),
        }
    }
    pub fn take() -> Rc<Network> {
        let mut network = NETWORK.lock();
        let network = network.get_or_insert_with(|| Rc::new(Self::new()));
        network.clone()
    }
    pub fn register_interface(&self, iface: Weak<dyn NetworkInterface>) {
        let mut interfaces = self.interfaces.lock();
        interfaces.push(iface)
    }
}
static NETWORK: Mutex<Option<Rc<Network>>> = Mutex::new(None);

pub async fn network_manager_thread() -> Result<()> {
    println!("Network manager is started!");
    let network = Network::take();

    loop {
        println!("Network interfaces:");
        let interfaces = network.interfaces.lock();
        for iface in &*interfaces {
            if let Some(iface) = iface.upgrade() {
                println!("{:?} {}", iface.ethernet_addr(), iface.name());
                for _ in 0..10 {
                    let arp_req = Box::pin(ArpPacket::request(
                        iface.ethernet_addr(),
                        IpV4Addr::new([10, 0, 2, 15]),
                        IpV4Addr::new([10, 0, 2, 2]),
                    ));
                    iface.queue_packet(arp_req.copy_into_slice())?;
                }
            }
        }
        TimeoutFuture::new_ms(1000).await;
    }
}
