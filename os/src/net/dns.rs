extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::executor::with_timeout_ms;
use crate::executor::yield_execution;
use crate::mutex::Mutex;
use crate::net::ip::IpV4Packet;
use crate::net::ip::IpV4Protocol;
use crate::net::manager::Network;
use crate::net::udp::UdpPacket;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::size_of;
use core::sync::atomic::AtomicU16;
use core::sync::atomic::Ordering;
use noli::mem::Sliceable;
use noli::net::IpV4Addr;

/*
c.f. https://datatracker.ietf.org/doc/html/rfc1035

e.g. response for hikalium.com



8, 104, 105, 107, 97, 108, 105, 117, 109,
3, 99, 111, 109,
0,
0, 1,
0, 1,
192, 12,  // 4.1.4. Message compression
    0, 1, // TYPE
    0, 1, // CLASS
    0, 0, 0, 225, // TTL
    0, 4, 185, 199, 108, 153,
192, 12,
    0, 1,
    0, 1,
    0, 0, 0, 225,
    0, 4, 185, 199, 111, 153,
192, 12,
    0, 1,
    0, 1,
    0, 0, 0, 225,
    0, 4, 185, 199, 110, 153,
192, 12, 0, 1, 0, 1,
    0, 0,
    0, 225,
    0, 4, 185, 199, 109, 153,
69, 209, 4, 13
*/

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default, Debug)]
pub struct DnsPacket {
    pub udp: UdpPacket,
    transaction_id: [u8; 2],
    flags: [u8; 2],
    num_questions: [u8; 2],
    num_answers: [u8; 2],
    num_authority_rr: [u8; 2],
    num_additional_rr: [u8; 2],
}
const _: () = assert!(size_of::<DnsPacket>() - size_of::<UdpPacket>() == 12);
impl DnsPacket {
    fn num_questions(&self) -> usize {
        u16::from_be_bytes(self.num_questions) as usize
    }
    fn num_answers(&self) -> usize {
        u16::from_be_bytes(self.num_answers) as usize
    }
}
unsafe impl Sliceable for DnsPacket {}

pub const PORT_DNS_SERVER: u16 = 53;

pub fn create_dns_query_packet(query_host_name: &str) -> Result<Vec<u8>> {
    let dns = DnsPacket {
        flags: [0x01, 0x20],
        num_questions: [0x00, 0x01],

        ..Default::default()
    };
    let mut query = Vec::new();
    query.extend(dns.as_slice());
    for s in query_host_name.trim().split('.') {
        let s = s.as_bytes();
        query.push(s.len() as u8);
        query.extend(s);
    }
    query.extend([0, 0, 1, 0, 1]);
    query.resize(512 - query.len(), 0);
    Ok(query)
}

static PENDING_QUERIES: Mutex<BTreeMap<u16, Option<Vec<DnsResponseEntry>>>> =
    Mutex::new(BTreeMap::new());
static NEXT_TRANSACTION_ID: AtomicU16 = AtomicU16::new(1);

pub fn parse_dns_response(dns_packet: &[u8]) -> Result<()> {
    let dns_header = DnsPacket::from_slice(dns_packet)?;
    let transaction_id = u16::from_be_bytes(dns_header.transaction_id);
    let dns_res = &dns_packet[size_of::<DnsPacket>()..];
    let mut it = dns_res.iter();
    let it = it.by_ref();
    let mut name = String::new();
    for _ in 0..dns_header.num_questions() {
        // [rfc1035]
        // 4.1.2. Question section format
        // [QNAME; N] [QTYPE; 2] [QCLASS; 2]
        // N can be odd (no padding)
        loop {
            if let Some(len) = it.next() {
                if *len == 0 {
                    it.advance_by(4).or(Err(Error::Failed("Invalid format")))?;
                    break;
                }
                let s = it.take(*len as usize).cloned().collect::<Vec<u8>>();
                let s =
                    String::from_utf8(s).or(Err(Error::Failed("Cannot parse the dns response")))?;
                name.push_str(&format!("{s}."));
            } else {
                return Err(Error::Failed(
                    "Failed to parse DNS response questions section",
                ));
            }
        }
    }
    let mut result = Vec::new();
    for _ in 0..dns_header.num_answers() {
        it.advance_by(12).or(Err(Error::Failed("Invalid format")))?;
        let ipv4_addr: [u8; 4] = it
            .take(4)
            .cloned()
            .collect::<Vec<u8>>()
            .try_into()
            .or(Err(Error::Failed("failed")))?;
        let name = name.clone();
        let addr = *IpV4Addr::from_slice(&ipv4_addr)?;
        result.push(DnsResponseEntry::A { name, addr })
    }
    if PENDING_QUERIES.lock().contains_key(&transaction_id) {
        PENDING_QUERIES.lock().insert(transaction_id, Some(result));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub enum DnsResponseEntry {
    A { name: String, addr: IpV4Addr },
}

pub async fn query_dns(query: &str) -> Result<Vec<DnsResponseEntry>> {
    let network = Network::take();
    let server = network
        .dns()
        .ok_or(Error::Failed("DNS server address is not available yet"))?;
    let transaction_id = NEXT_TRANSACTION_ID.fetch_add(1, Ordering::SeqCst);
    let mut packet = create_dns_query_packet(query)?;
    {
        let ip = IpV4Packet::new(
            Default::default(),
            server,
            IpV4Addr::default(),
            IpV4Protocol::udp(),
            packet.len() - size_of::<IpV4Packet>(),
        );
        let mut udp = UdpPacket::default();
        udp.ip = ip;
        udp.set_dst_port(PORT_DNS_SERVER);
        udp.set_src_port(53);
        udp.set_data_size(packet.len() - size_of::<UdpPacket>())?;
        let packet = DnsPacket::from_slice_mut(&mut packet)?;
        packet.udp = udp;
        packet.transaction_id = transaction_id.to_be_bytes();
    }
    PENDING_QUERIES.lock().insert(transaction_id, None);
    network.send_ip_packet(packet.into());
    with_timeout_ms(
        async {
            loop {
                yield_execution().await;
                let res = PENDING_QUERIES.lock().get(&transaction_id).cloned();
                if let Some(Some(res)) = res {
                    return res;
                }
            }
        },
        500,
    )
    .await
}
