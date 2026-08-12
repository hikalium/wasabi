extern crate alloc;

use crate::eth::EthernetAddr;
use crate::eth::EthernetHeader;
use crate::eth::EthernetType;
use crate::ip::IpV4Addr;
use crate::ip::IpV4Packet;
use crate::ip::IpV4Protocol;
use crate::result::Result;
use crate::slice::Sliceable;
use crate::udp::UdpPacket;
use crate::udp::UDP_PORT_DHCP_CLIENT;
use crate::udp::UDP_PORT_DHCP_SERVER;
use alloc::vec::Vec;
use core::mem::size_of;
use core::mem::MaybeUninit;

// https://datatracker.ietf.org/doc/html/rfc2132
// 3.3. Subnet Mask (len = 4)
pub const DHCP_OPT_NETMASK: u8 = 1;
// 3.5. Router Option (len = 4 * n where n >= 1)
pub const DHCP_OPT_ROUTER: u8 = 3;
// 3.8. Domain Name Server Option (len = 4 * n where n >= 1)
pub const DHCP_OPT_DNS: u8 = 6;
// 9.1. Requested IP Address (len = 4)
pub const DHCP_OPT_REQUESTED_IP: u8 = 50;
// 9.6. DHCP Message Type (len = 1)
pub const DHCP_OPT_MESSAGE_TYPE: u8 = 53;
// 9.7. Server Identifier (len = 4)
pub const DHCP_OPT_SERVER_ID: u8 = 54;
// Fixed length (1-byte) options
pub const DHCP_OPT_MESSAGE_TYPE_PADDING: u8 = 0;
pub const DHCP_OPT_MESSAGE_TYPE_END: u8 = 255;
// Variable length ((2 + len) bytes) options
pub const DHCP_OPT_MESSAGE_TYPE_DISCOVER: u8 = 1;
pub const DHCP_OPT_MESSAGE_TYPE_OFFER: u8 = 2;
pub const DHCP_OPT_MESSAGE_TYPE_REQUEST: u8 = 3;
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
    pub fn is_boot_reply(&self) -> bool {
        self.op == DHCP_OP_BOOTREPLY
    }
    /// Your Ip ADDRess
    pub fn yiaddr(&self) -> IpV4Addr {
        self.yiaddr
    }
    /// Client's Hardware ADDRess
    pub fn chaddr(&self) -> EthernetAddr {
        self.chaddr
    }
    /// Serialise a broadcast BOOTREQUEST carrying `options` as one
    /// complete Ethernet frame. `options` is the option field that
    /// follows the magic cookie, so it has to end with an END (255).
    fn build(src_eth_addr: EthernetAddr, options: &[u8]) -> Result<Vec<u8>> {
        let mut this = Self::default();
        // eth
        let eth = EthernetHeader::new(
            EthernetAddr::broadcast(),
            src_eth_addr,
            EthernetType::ip_v4(),
        );
        // ip
        let data_length =
            size_of::<Self>() - size_of::<IpV4Packet>() + options.len();
        let ip = IpV4Packet::new(
            eth,
            IpV4Addr::broadcast(),
            IpV4Addr::default(),
            IpV4Protocol::udp(),
            data_length,
        );
        // udp
        this.udp.ip = ip;
        this.udp.set_src_port(UDP_PORT_DHCP_CLIENT);
        this.udp.set_dst_port(UDP_PORT_DHCP_SERVER);
        this.udp.set_data_size(data_length)?;
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
        this.udp.ip.recompute_checksum();
        let mut frame = Vec::with_capacity(size_of::<Self>() + options.len());
        frame.extend_from_slice(this.as_slice());
        frame.extend_from_slice(options);
        Ok(frame)
    }
    /// DHCPDISCOVER: ask every server on the link for an address. The
    /// message type option is what makes this a DHCP message; without it
    /// servers read the packet as a plain BOOTP request and answer only
    /// from a static table, if at all.
    pub fn discover(src_eth_addr: EthernetAddr) -> Result<Vec<u8>> {
        Self::build(
            src_eth_addr,
            &[
                DHCP_OPT_MESSAGE_TYPE,
                1,
                DHCP_OPT_MESSAGE_TYPE_DISCOVER,
                DHCP_OPT_MESSAGE_TYPE_END,
            ],
        )
    }
    /// DHCPREQUEST: ask `server_id` to commit `requested_ip`, the address
    /// it put in the yiaddr of its OFFER. An offer is not a lease until
    /// the server acks this.
    pub fn request(
        src_eth_addr: EthernetAddr,
        requested_ip: IpV4Addr,
        server_id: IpV4Addr,
    ) -> Result<Vec<u8>> {
        let ip = requested_ip.bytes();
        let sid = server_id.bytes();
        Self::build(
            src_eth_addr,
            &[
                DHCP_OPT_MESSAGE_TYPE,
                1,
                DHCP_OPT_MESSAGE_TYPE_REQUEST,
                DHCP_OPT_REQUESTED_IP,
                4,
                ip[0],
                ip[1],
                ip[2],
                ip[3],
                DHCP_OPT_SERVER_ID,
                4,
                sid[0],
                sid[1],
                sid[2],
                sid[3],
                DHCP_OPT_MESSAGE_TYPE_END,
            ],
        )
    }
}
impl Default for DhcpPacket {
    fn default() -> Self {
        // SAFETY: This is safe since DhcpPacket is valid as a data for any
        // contents
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}
unsafe impl Sliceable for DhcpPacket {}
