extern crate alloc;

use crate::error::Result;
use crate::net::udp::UdpPacket;
use crate::util::Sliceable;
use alloc::vec::Vec;
use core::mem::size_of;
use noli::net::IpV4Addr;

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
#[derive(Copy, Clone, Default)]
pub struct DnsPacket {
    udp: UdpPacket,
    transaction_id: u16,
    flags: u16,
    num_questions: u16,
    num_answers: u16,
    num_authority_rr: u16,
    num_additional_rr: u16,
}
const _: () = assert!(size_of::<DnsPacket>() - size_of::<UdpPacket>() == 12);
impl DnsPacket {}
unsafe impl Sliceable for DnsPacket {}

const PORT_DNS_SERVER: u16 = 53;

pub fn create_dns_query_packet(query_host_name: &str) -> Result<Vec<u8>> {
    let dns = DnsPacket {
        flags: 0x2001,
        num_questions: 0x0100,
        ..Default::default()
    };
    let mut query = Vec::new();
    query.extend(dns.as_slice());
    for s in query_host_name.trim().split('.') {
        query.push(s.len() as u8);
        query.extend(s.as_bytes());
    }
    query.extend([0, 0, 1, 0, 1]);
    Ok(query)
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
