extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::info;
use crate::net::ip::IpV4Addr;
use crate::net::udp::UdpPacket;
use crate::util::Sliceable;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::size_of;

/*
query:
4f 15 01 20 00 01 00 00  00 00 00 01 07 65 78 61
6d 70 6c 65 03 63 6f 6d  00 00 01 00 01 00 00 29
04 d0 00 00 00 00 00 0c  00 0a 00 08 9f 8c ab 51
7a f5 f9 0f

response:
0c ec 81 80 00 01 00 01  00 00 00 01 07 65 78 61
6d 70 6c 65 03 63 6f 6d  00 00 01 00 01 c0 0c 00
01 00 01 00 00 09 98 00  04 5d b8 d7 0e 00 00 29
04 d0 00 00 00 00 00 00
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
impl DnsPacket {}
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

#[derive(Debug, Clone)]
pub enum DnsResponseEntry {
    A { name: String, addr: IpV4Addr },
}

pub fn parse_dns_response(dns_packet: &[u8]) -> Result<Vec<DnsResponseEntry>> {
    info!("dns_client: {dns_packet:?}");
    let dns_header = DnsPacket::from_slice(dns_packet)?;
    info!("dns_header: {dns_header:?}");

    Err(Error::Failed("WIP"))
}
/*
int main(int argc, char** argv) {

  struct sockaddr_in client_address;
  socklen_t client_addr_len = sizeof(client_address);
  uint8_t buf[4096];
  ssize_t recv_size =
      recvfrom(socket_fd, (char*)buf, sizeof(buf), 0,
               (struct sockaddr*)&client_address, &client_addr_len);
  if (recv_size == -1) {
    panic("error: recvfrom returned -1\n");
  }
  Print("Received size: ");
  PrintNum(recv_size);
  Print("\n");

  for (int i = 0; i < recv_size; i++) {
    PrintHex8ZeroFilled(buf[i]);
    Print((i & 0xF) == 0xF ? "\n" : " ");
  }
  Print("\n");

  struct DNSMessage* dns = (struct DNSMessage*)buf;
  Print("Num of answer RRs: ");
  PrintNum(htons(dns->num_answers));
  Print("\n");

  uint8_t* p = buf + sizeof(struct DNSMessage);
  for (int i = 0; i < htons(dns->num_questions); i++) {
    Print("Query: ");
    while (*p) {
      write(1, &p[1], *p);
      p += *p + 1;
      if (*p) {
        write(1, ".", 1);
      }
    }
    p++;
    p += 2;  // Type
    p += 2;  // Class
    Print("\n");
  }

  Print("Answers:\n");
  for (int i = 0; i < htons(dns->num_answers); i++) {
    p += 2;  // pointer
    p += 2;  // Type
    p += 2;  // Class
    p += 4;  // TTL
    p += 2;  // len (4)
    PrintIPv4Addr(*(uint32_t*)p);
    p += 4;
    Print("\n");
  }

  return 0;
}
*/
