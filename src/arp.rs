extern crate alloc;

use crate::eth::EthernetAddr;
use crate::eth::EthernetHeader;
use crate::eth::EthernetType;
use crate::ip::IpV4Addr;
use crate::slice::Sliceable;
use core::mem::size_of;

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct ArpPacket {
    eth_header: EthernetHeader,
    hw_type: [u8; 2],    // 0x0001 for Ethernet
    proto_type: [u8; 2], // 0x0800 for IPv4
    hw_addr_size: u8,    // 6
    proto_addr_size: u8, // 4
    op: [u8; 2],         // 1 = request, 2 = reply
    sender_mac: EthernetAddr,
    sender_ip: IpV4Addr,
    target_mac: EthernetAddr,
    target_ip: IpV4Addr,
}
const _: () = assert!(size_of::<ArpPacket>() == 42);
unsafe impl Sliceable for ArpPacket {}

impl ArpPacket {
    pub fn request(
        src_eth: EthernetAddr,
        src_ip: IpV4Addr,
        dst_ip: IpV4Addr,
    ) -> Self {
        Self {
            eth_header: EthernetHeader::new(
                EthernetAddr::broadcast(),
                src_eth,
                EthernetType::arp(),
            ),
            hw_type: [0x00, 0x01],
            proto_type: [0x08, 0x00],
            hw_addr_size: 6,
            proto_addr_size: 4,
            op: [0x00, 0x01],
            sender_mac: src_eth,
            sender_ip: src_ip,
            target_mac: EthernetAddr::zero(),
            target_ip: dst_ip,
        }
    }
    /// Gratuitous ARP: an ARP request where sender_ip == target_ip,
    /// used to announce our own IP-to-MAC binding to the network.
    pub fn gratuitous(our_eth: EthernetAddr, our_ip: IpV4Addr) -> Self {
        Self::request(our_eth, our_ip, our_ip)
    }
    pub fn op_is_request(&self) -> bool {
        let op = self.op;
        op == [0x00, 0x01]
    }
    pub fn target_ip(&self) -> IpV4Addr {
        self.target_ip
    }
    pub fn sender_ip(&self) -> IpV4Addr {
        self.sender_ip
    }
    pub fn sender_mac(&self) -> EthernetAddr {
        self.sender_mac
    }
    pub fn is_request_for(&self, ip: IpV4Addr) -> bool {
        self.op_is_request() && self.target_ip == ip
    }
    /// Build an ARP reply (op = 2) addressed back to the original requester.
    /// `our_eth` is the MAC we want the requester to associate with the
    /// IP they were asking about (= the original target_ip).
    pub fn reply_to(&self, our_eth: EthernetAddr) -> ArpPacket {
        ArpPacket {
            eth_header: EthernetHeader::new(
                self.sender_mac,
                our_eth,
                EthernetType::arp(),
            ),
            hw_type: [0x00, 0x01],
            proto_type: [0x08, 0x00],
            hw_addr_size: 6,
            proto_addr_size: 4,
            op: [0x00, 0x02],
            sender_mac: our_eth,
            sender_ip: self.target_ip,
            target_mac: self.sender_mac,
            target_ip: self.sender_ip,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn arp_packet_size_is_42() {
        assert_eq!(size_of::<ArpPacket>(), 42);
    }

    #[test_case]
    fn arp_request_layout() {
        let src_mac = EthernetAddr::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        let src_ip = IpV4Addr::new([10, 10, 10, 83]);
        let dst_ip = IpV4Addr::new([10, 10, 10, 1]);
        let pkt = ArpPacket::request(src_mac, src_ip, dst_ip);
        let bytes = pkt.as_slice().to_vec();

        // Ethernet header: dst = broadcast, src = our MAC, type = ARP.
        assert_eq!(&bytes[0..6], &[0xFF; 6]);
        assert_eq!(&bytes[6..12], &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(&bytes[12..14], &[0x08, 0x06]);

        // ARP body.
        assert_eq!(&bytes[14..16], &[0x00, 0x01]); // hw_type = Ethernet
        assert_eq!(&bytes[16..18], &[0x08, 0x00]); // proto_type = IPv4
        assert_eq!(bytes[18], 6); // hw_addr_size
        assert_eq!(bytes[19], 4); // proto_addr_size
        assert_eq!(&bytes[20..22], &[0x00, 0x01]); // op = request

        assert_eq!(&bytes[22..28], &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(&bytes[28..32], &[10, 10, 10, 83]);
        assert_eq!(&bytes[32..38], &[0x00; 6]); // target_mac unknown
        assert_eq!(&bytes[38..42], &[10, 10, 10, 1]);
    }

    #[test_case]
    fn arp_gratuitous_target_ip_equals_sender_ip() {
        let mac = EthernetAddr::new([1, 2, 3, 4, 5, 6]);
        let ip = IpV4Addr::new([10, 10, 10, 83]);
        let pkt = ArpPacket::gratuitous(mac, ip);
        let bytes = pkt.as_slice().to_vec();
        // sender_ip at 28..32, target_ip at 38..42, both equal 10.10.10.83.
        assert_eq!(&bytes[28..32], &[10, 10, 10, 83]);
        assert_eq!(&bytes[38..42], &[10, 10, 10, 83]);
    }

    #[test_case]
    fn arp_is_request_for_match_and_mismatch() {
        let our_ip = IpV4Addr::new([10, 10, 10, 83]);
        let other_ip = IpV4Addr::new([10, 10, 10, 1]);
        let req = ArpPacket::request(
            EthernetAddr::new([1, 2, 3, 4, 5, 6]),
            other_ip,
            our_ip,
        );
        assert!(req.is_request_for(our_ip));
        assert!(!req.is_request_for(other_ip));
    }

    #[test_case]
    fn arp_reply_to_layout() {
        let peer_mac = EthernetAddr::new([0xAA; 6]);
        let peer_ip = IpV4Addr::new([10, 10, 10, 1]);
        let our_ip = IpV4Addr::new([10, 10, 10, 83]);
        let our_mac = EthernetAddr::new([0x11; 6]);

        let req = ArpPacket::request(peer_mac, peer_ip, our_ip);
        let reply = req.reply_to(our_mac);
        let bytes = reply.as_slice().to_vec();

        // Ethernet: dst = peer, src = us, type = ARP.
        assert_eq!(&bytes[0..6], &[0xAA; 6]);
        assert_eq!(&bytes[6..12], &[0x11; 6]);
        assert_eq!(&bytes[12..14], &[0x08, 0x06]);
        // op = 2 (reply)
        assert_eq!(&bytes[20..22], &[0x00, 0x02]);
        // sender = us, target = peer
        assert_eq!(&bytes[22..28], &[0x11; 6]);
        assert_eq!(&bytes[28..32], &[10, 10, 10, 83]);
        assert_eq!(&bytes[32..38], &[0xAA; 6]);
        assert_eq!(&bytes[38..42], &[10, 10, 10, 1]);
    }
}
