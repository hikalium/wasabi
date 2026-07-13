extern crate alloc;

use crate::checksum::InternetChecksum;
use crate::eth::EthernetAddr;
use crate::eth::EthernetHeader;
use crate::eth::EthernetType;
use crate::ip::IpV4Addr;
use crate::ip::IpV4Packet;
use crate::ip::IpV4Protocol;
use crate::result::Result;
use crate::slice::Sliceable;
use alloc::vec::Vec;
use core::mem::size_of;

#[repr(transparent)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct IcmpType(pub u8);
impl IcmpType {
    pub const fn echo_request() -> Self {
        Self(8)
    }
    pub const fn echo_reply() -> Self {
        Self(0)
    }
}

// ICMP fixed header for Echo Request/Reply, prefixed by the IPv4 packet
// (which itself prefixes the Ethernet header). Echo data follows the 8
// header bytes and is variable-length.
#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct IcmpPacket {
    pub ip: IpV4Packet,
    icmp_type: IcmpType,
    code: u8,
    csum: InternetChecksum,
    identifier: [u8; 2],
    sequence: [u8; 2],
}
const _: () = assert!(size_of::<IcmpPacket>() - size_of::<IpV4Packet>() == 8);
unsafe impl Sliceable for IcmpPacket {}

impl IcmpPacket {
    pub fn icmp_type(&self) -> IcmpType {
        self.icmp_type
    }
    pub fn is_echo_request(&self) -> bool {
        self.icmp_type() == IcmpType::echo_request()
    }
}

/// Build an ICMP Echo Reply for an incoming Echo Request frame.
///
/// `request` must be the entire received Ethernet+IPv4+ICMP frame
/// (header + data). The returned bytes are the full reply frame ready
/// to be wrapped into an NTB and pushed onto bulk-OUT.
pub fn echo_reply_from_request(
    request: &[u8],
    our_eth: EthernetAddr,
    our_ip: IpV4Addr,
) -> Result<Vec<u8>> {
    if request.len() < size_of::<IcmpPacket>() {
        return Err("ICMP request too short");
    }
    let req = IcmpPacket::copy_from_slice(&request[..size_of::<IcmpPacket>()])?;
    if !req.is_echo_request() {
        return Err("ICMP packet is not an echo request");
    }

    // Take the request bytes as a starting point — the ICMP id/seq and
    // any echo data are reused verbatim — then patch the headers.
    let mut reply = request.to_vec();

    // Rewrite Ethernet + IPv4 headers in one shot via the typed builder.
    let new_eth =
        EthernetHeader::new(req.ip.eth.src(), our_eth, EthernetType::ip_v4());
    let payload_len = request.len() - size_of::<IpV4Packet>();
    let mut new_ip = IpV4Packet::new(
        new_eth,
        req.ip.src(),
        our_ip,
        IpV4Protocol::icmp(),
        payload_len,
    );
    new_ip.recompute_checksum();
    reply[..size_of::<IpV4Packet>()].copy_from_slice(new_ip.as_slice());

    // Patch the ICMP header: type 8 -> 0, zero the checksum then recompute.
    let icmp_off = size_of::<IpV4Packet>();
    reply[icmp_off] = IcmpType::echo_reply().0;
    reply[icmp_off + 1] = 0; // code (echo: always 0)
    reply[icmp_off + 2] = 0; // csum hi (clear before recompute)
    reply[icmp_off + 3] = 0; // csum lo

    // identifier (icmp_off+4..6) and sequence (icmp_off+6..8) preserved.

    let csum = InternetChecksum::calc(&reply[icmp_off..]);
    let csum_bytes = csum.bytes();
    reply[icmp_off + 2] = csum_bytes[0];
    reply[icmp_off + 3] = csum_bytes[1];

    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::InternetChecksumGenerator;

    #[test_case]
    fn icmp_packet_extra_size_is_8() {
        assert_eq!(size_of::<IcmpPacket>() - size_of::<IpV4Packet>(), 8);
    }

    // A fabricated 64-byte ICMP echo request (14 eth + 20 ip + 8 icmp +
    // 22 data) used as a stand-in for a `ping -s 22` packet.
    fn fake_echo_request() -> Vec<u8> {
        let mut req = alloc::vec![0u8; 14 + 20 + 8 + 22];
        // Ethernet: dst = our test MAC, src = host MAC, type = IPv4.
        req[0..6].copy_from_slice(&[0x11, 0x11, 0x11, 0x11, 0x11, 0x11]); // dst
                                                                          // src
        req[6..12].copy_from_slice(&[0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA]);
        req[12..14].copy_from_slice(&[0x08, 0x00]); // EtherType IPv4
                                                    // IPv4 header.
        req[14] = 0x45; // ver=4, ihl=5
        req[15] = 0; // dscp/ecn
        req[16..18].copy_from_slice(&((20 + 8 + 22) as u16).to_be_bytes());
        req[18..20].copy_from_slice(&[0x00, 0x00]); // ident
        req[20..22].copy_from_slice(&[0x00, 0x00]); // flags/frag
        req[22] = 0xff; // ttl
        req[23] = 1; // protocol = ICMP
                     // csum bytes 24..26 = 0; not validated by our reply path.
        req[26..30].copy_from_slice(&[10, 10, 10, 1]); // src IP (host)
        req[30..34].copy_from_slice(&[10, 10, 10, 83]); // dst IP (us)
                                                        // ICMP header.
        req[34] = 8; // type = echo request
        req[35] = 0; // code
        req[36..38].copy_from_slice(&[0, 0]); // csum (don't care)
        req[38..40].copy_from_slice(&[0xBE, 0xEF]); // identifier
        req[40..42].copy_from_slice(&[0x00, 0x05]); // sequence
                                                    // 22 bytes of echo data.
        for (i, b) in req[42..].iter_mut().enumerate() {
            *b = i as u8;
        }
        req
    }

    #[test_case]
    fn echo_reply_swaps_addresses_and_sets_type() {
        let our_mac = EthernetAddr::new([0x33, 0x33, 0x33, 0x33, 0x33, 0x33]);
        let our_ip = IpV4Addr::new([10, 10, 10, 83]);
        let req = fake_echo_request();
        let reply = echo_reply_from_request(&req, our_mac, our_ip).unwrap();

        // Ethernet: dst = original src, src = our MAC, type = IPv4.
        assert_eq!(&reply[0..6], &[0xAA; 6]);
        assert_eq!(&reply[6..12], &[0x33; 6]);
        assert_eq!(&reply[12..14], &[0x08, 0x00]);
        // IPv4: src = us, dst = original src IP.
        assert_eq!(&reply[26..30], &[10, 10, 10, 83]);
        assert_eq!(&reply[30..34], &[10, 10, 10, 1]);
        // ICMP: type = 0 (reply), code = 0; ident/seq preserved.
        assert_eq!(reply[34], 0);
        assert_eq!(reply[35], 0);
        assert_eq!(&reply[38..40], &[0xBE, 0xEF]);
        assert_eq!(&reply[40..42], &[0x00, 0x05]);
        // Echo data preserved.
        assert_eq!(&reply[42..], &req[42..]);
        // Reply length matches request length.
        assert_eq!(reply.len(), req.len());
    }

    #[test_case]
    fn echo_reply_checksums_self_check() {
        let our_mac = EthernetAddr::new([0x33, 0x33, 0x33, 0x33, 0x33, 0x33]);
        let our_ip = IpV4Addr::new([10, 10, 10, 83]);
        let req = fake_echo_request();
        let reply = echo_reply_from_request(&req, our_mac, our_ip).unwrap();

        // IP header self-check: feeding the header (with its csum) back
        // through the generator yields 0x0000.
        let mut g = InternetChecksumGenerator::new();
        g.feed(&reply[14..34]);
        assert_eq!(g.checksum(), InternetChecksum::default());

        // ICMP self-check: feeding the entire ICMP segment yields 0x0000.
        let mut g = InternetChecksumGenerator::new();
        g.feed(&reply[34..]);
        assert_eq!(g.checksum(), InternetChecksum::default());
    }
}
