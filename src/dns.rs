extern crate alloc;

use crate::eth::EthernetAddr;
use crate::eth::EthernetHeader;
use crate::eth::EthernetType;
use crate::ip::IpV4Addr;
use crate::ip::IpV4Packet;
use crate::ip::IpV4Protocol;
use crate::mutex::Mutex;
use crate::result::Result;
use crate::slice::Sliceable;
use crate::udp::UdpPacket;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::mem::size_of;

pub const PORT_DNS: u16 = 53;
// Fixed ephemeral source port for our queries. One query is outstanding
// at a time, so a single port is enough.
const DNS_SRC_PORT: u16 = 50000;

// A DNS message header (RFC 1035 §4.1.1) preceded by the lower-layer
// headers, so the whole query serialises as one Ethernet frame the same
// way `DhcpPacket` does.
#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
struct DnsHeader {
    udp: UdpPacket,
    transaction_id: [u8; 2],
    flags: [u8; 2],
    num_questions: [u8; 2],
    num_answers: [u8; 2],
    num_authority_rr: [u8; 2],
    num_additional_rr: [u8; 2],
}
const _: () = assert!(size_of::<DnsHeader>() - size_of::<UdpPacket>() == 12);
unsafe impl Sliceable for DnsHeader {}

// DNS reply frames stashed by the NCM rx path for the waiting `dns`
// command to pick up. A queue rather than a single slot because several
// queries can be outstanding at once, and their replies would otherwise
// overwrite each other before anyone looked.
static DNS_RX: Mutex<VecDeque<Vec<u8>>> = Mutex::new(VecDeque::new());
// A burst is not bounded any more, so this is the one place replies can
// still be lost: it has to hold whatever arrives between two passes of
// the waiting command. Matched to the tx queue depth, since that is
// what bounds how many can be in flight.
const DNS_RX_MAX: usize = 256;

/// Called from the NCM rx path for any UDP frame whose source port is
/// 53. Stores the raw frame so a pending query can parse it.
pub fn deliver_response(frame: &[u8]) {
    let mut rx = DNS_RX.lock();
    if rx.len() >= DNS_RX_MAX {
        rx.pop_front();
    }
    rx.push_back(frame.to_vec());
}

/// Drop any stale replies before sending a new query.
pub fn clear_response() {
    DNS_RX.lock().clear();
}

/// Take the oldest stored reply frame, if one has arrived.
pub fn take_response() -> Option<Vec<u8>> {
    DNS_RX.lock().pop_front()
}

/// Build a DNS A-record query for `hostname` as a complete Ethernet
/// frame. The frame is addressed at layer 2 to `next_hop_mac` (the
/// gateway when `server` is off-subnet) but carries `server` as the IP
/// destination, so it rides out through the router unchanged.
pub fn build_query(
    our_mac: EthernetAddr,
    our_ip: IpV4Addr,
    next_hop_mac: EthernetAddr,
    server: IpV4Addr,
    hostname: &str,
    txid: u16,
) -> Result<Vec<u8>> {
    // Question section: length-prefixed labels, a root label (0), then
    // QTYPE=A(1) and QCLASS=IN(1).
    let mut question = Vec::new();
    for label in hostname.trim().split('.') {
        if label.is_empty() {
            continue;
        }
        let b = label.as_bytes();
        if b.len() > 63 {
            return Err("dns: label too long");
        }
        question.push(b.len() as u8);
        question.extend_from_slice(b);
    }
    question.push(0);
    question.extend_from_slice(&[0, 1, 0, 1]);

    // Bytes after the IP header = UDP header (8) + DNS header (12) +
    // question. This is both the IP payload length and the UDP length.
    let udp_payload_len = 12 + question.len();
    let after_ip =
        (size_of::<UdpPacket>() - size_of::<IpV4Packet>()) + udp_payload_len;

    let eth = EthernetHeader::new(next_hop_mac, our_mac, EthernetType::ip_v4());
    let ip =
        IpV4Packet::new(eth, server, our_ip, IpV4Protocol::udp(), after_ip);

    let mut hdr = DnsHeader::default();
    hdr.udp.ip = ip;
    hdr.udp.set_src_port(DNS_SRC_PORT);
    hdr.udp.set_dst_port(PORT_DNS);
    hdr.udp.set_data_size(after_ip)?;
    hdr.transaction_id = txid.to_be_bytes();
    // 0x0120 = standard query, recursion desired.
    hdr.flags = [0x01, 0x20];
    hdr.num_questions = [0x00, 0x01];
    hdr.udp.ip.recompute_checksum();

    let mut frame = Vec::with_capacity(size_of::<DnsHeader>() + question.len());
    frame.extend_from_slice(hdr.as_slice());
    frame.extend_from_slice(&question);
    Ok(frame)
}

/// Parse a DNS response frame, returning its transaction id and every
/// IPv4 address found in an A record. Returns `None` if the frame is too
/// short or malformed.
pub fn parse_response(frame: &[u8]) -> Option<(u16, Vec<IpV4Addr>)> {
    let base = size_of::<DnsHeader>();
    if frame.len() < base {
        return None;
    }
    let hdr = DnsHeader::copy_from_slice(&frame[..base]).ok()?;
    let txid = u16::from_be_bytes(hdr.transaction_id);
    let num_q = u16::from_be_bytes(hdr.num_questions) as usize;
    let num_a = u16::from_be_bytes(hdr.num_answers) as usize;

    // The DNS message body (question + answers) follows the fixed
    // header, which `DnsHeader` already spans.
    let mut i = base;
    for _ in 0..num_q {
        i = skip_name(frame, i)?;
        i = i.checked_add(4)?; // QTYPE + QCLASS
    }
    let mut addrs = Vec::new();
    for _ in 0..num_a {
        i = skip_name(frame, i)?;
        // TYPE(2) CLASS(2) TTL(4) RDLENGTH(2) then RDATA.
        if i + 10 > frame.len() {
            break;
        }
        let rtype = u16::from_be_bytes([frame[i], frame[i + 1]]);
        let rdlen = u16::from_be_bytes([frame[i + 8], frame[i + 9]]) as usize;
        i += 10;
        if i + rdlen > frame.len() {
            break;
        }
        if rtype == 1 && rdlen == 4 {
            addrs.push(IpV4Addr::new([
                frame[i],
                frame[i + 1],
                frame[i + 2],
                frame[i + 3],
            ]));
        }
        i += rdlen;
    }
    Some((txid, addrs))
}

// Advance past a DNS name starting at `i`: a run of length-prefixed
// labels ended by a 0 byte, or a 2-byte compression pointer (top two
// bits of the length set, RFC 1035 §4.1.4).
fn skip_name(frame: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let len = *frame.get(i)?;
        if len & 0xc0 == 0xc0 {
            return Some(i + 2);
        }
        if len == 0 {
            return Some(i + 1);
        }
        i = i.checked_add(1 + len as usize)?;
    }
}
