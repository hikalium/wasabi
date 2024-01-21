use crate::net::checksum::InternetChecksum;
use crate::net::ip::IpV4Packet;
use crate::util::Sliceable;

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct TcpPacket {
    pub ip: IpV4Packet,
    src_port: [u8; 2], // optional
    dst_port: [u8; 2],
    data_size: [u8; 2],
    seq_num: [u8; 4],
    ack_num: [u8; 4],
    flags: [u8; 2],
    window: [u8; 2],
    csum: InternetChecksum,
    urgent_ptr: [u8; 2],
    // Options follow...
    // [type: u8], [len: u8], [data: [u8; len]], ...
}

impl TcpPacket {
    pub fn src_port(&self) -> u16 {
        u16::from_be_bytes(self.src_port)
    }
    pub fn dst_port(&self) -> u16 {
        u16::from_be_bytes(self.dst_port)
    }
    pub fn seq_num(&self) -> u32 {
        u32::from_be_bytes(self.seq_num)
    }
    pub fn ack_num(&self) -> u32 {
        u32::from_be_bytes(self.ack_num)
    }
    pub fn header_len(&self) -> usize {
        4 * (self.flags[0] >> 4) as usize
    }
    pub fn flags(&self) -> u32 {
        u32::from_be_bytes(self.ack_num) & 0x0777
    }
    pub fn is_syn(&self) -> bool {
        self.flags() & 0b10 != 0
    }
    pub fn window(&self) -> u32 {
        u32::from_be_bytes(self.ack_num)
    }
}
unsafe impl Sliceable for TcpPacket {}
