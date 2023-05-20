use crate::net::udp::UdpPacket;
use crate::net::udp::UDP_PORT_DHCP_CLIENT;
use crate::net::udp::UDP_PORT_DHCP_SERVER;
use crate::net::EthernetAddr;
use crate::net::EthernetHeader;
use crate::net::EthernetType;
use crate::net::InternetChecksum;
use crate::net::IpV4Addr;
use crate::net::IpV4Packet;
use crate::net::IpV4Protocol;
use crate::util::Sliceable;
use core::mem::size_of;
use core::mem::MaybeUninit;

// https://datatracker.ietf.org/doc/html/rfc2132
// 3.3. Subnet Mask (len = 4)
pub const DHCP_OPT_NETMASK: u8 = 1;
// 3.5. Router Option (len = 4 * n where n >= 1)
pub const DHCP_OPT_ROUTER: u8 = 3;
// 3.8. Domain Name Server Option (len = 4 * n where n >= 1)
pub const DHCP_OPT_DNS: u8 = 6;
// 9.6. DHCP Message Type (len = 1)
pub const DHCP_OPT_MESSAGE_TYPE: u8 = 53;
// Fixed length (1-byte) options
pub const DHCP_OPT_MESSAGE_TYPE_PADDING: u8 = 0;
pub const DHCP_OPT_MESSAGE_TYPE_END: u8 = 255;
// Variable length ((2 + len) bytes) options
pub const DHCP_OPT_MESSAGE_TYPE_DISCOVER: u8 = 1;
pub const DHCP_OPT_MESSAGE_TYPE_OFFER: u8 = 2;
pub const DHCP_OPT_MESSAGE_TYPE_ACK: u8 = 5;

// https://datatracker.ietf.org/doc/html/rfc2131#section-2
pub const DHCP_OP_BOOTREQUEST: u8 = 1; // CLIENT -> SERVER
pub const DHCP_OP_BOOTREPLY: u8 = 2; // SERVER -> CLIENT

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone)]
pub struct DhcpPacket {
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
    pub fn op(&self) -> u8 {
        self.op
    }
    pub fn yiaddr(&self) -> IpV4Addr {
        self.yiaddr
    }
    pub fn request(src_eth_addr: EthernetAddr) -> Self {
        let mut this = Self::default();
        // eth
        let eth = EthernetHeader::new(
            EthernetAddr::broardcast(),
            src_eth_addr,
            EthernetType::ip_v4(),
        );
        // ip
        let data_length = size_of::<Self>() - size_of::<IpV4Packet>();
        let ip = IpV4Packet::new(
            eth,
            IpV4Addr::broardcast(),
            IpV4Addr::default(),
            IpV4Protocol::udp(),
            data_length,
        );
        // udp
        this.udp.ip = ip;
        this.udp.set_src_port(UDP_PORT_DHCP_CLIENT);
        this.udp.set_dst_port(UDP_PORT_DHCP_SERVER);
        this.udp
            .set_data_size((size_of::<Self>() - size_of::<IpV4Packet>()) as u16);
        // udp checksum is omitted (set to zero) since it is optional
        // dhcp
        this.op = DHCP_OP_BOOTREQUEST;
        this.htype = 1;
        this.hlen = 6;
        this.xid = 0x1234;
        this.chaddr = src_eth_addr;
        // https://datatracker.ietf.org/doc/html/rfc2132#section-2
        // 2. BOOTP Extension/DHCP Option Field Format
        // > The value of the magic cookie is the 4 octet
        // dotted decimal 99.130.83.99 ... in network byte order.
        this.cookie = [99, 130, 83, 99];
        this.udp.ip.clear_checksum();
        this.udp.ip.set_checksum(InternetChecksum::calc(
            &this.udp.as_slice()[size_of::<EthernetHeader>()..size_of::<IpV4Packet>()],
        ));
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
