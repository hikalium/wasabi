use crate::net::checksum::InternetChecksum;
use crate::net::IpV4Packet;
use crate::util::Sliceable;

// https://datatracker.ietf.org/doc/html/rfc2131
// 4.1 Constructing and sending DHCP messages
pub const UDP_PORT_DHCP_SERVER: u16 = 67;
pub const UDP_PORT_DHCP_CLIENT: u16 = 68;

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default)]
pub struct UdpPacket {
    pub ip: IpV4Packet,
    src_port: [u8; 2], // optional
    dst_port: [u8; 2],
    data_size: [u8; 2],
    csum: InternetChecksum,
}
impl UdpPacket {
    pub fn src_port(&self) -> u16 {
        u16::from_be_bytes(self.src_port)
    }
    pub fn dst_port(&self) -> u16 {
        u16::from_be_bytes(self.dst_port)
    }
    pub fn set_src_port(&mut self, port: u16) {
        self.src_port = port.to_be_bytes();
    }
    pub fn set_dst_port(&mut self, port: u16) {
        self.dst_port = port.to_be_bytes();
    }
    pub fn set_data_size(&mut self, data_size: u16) {
        self.data_size = data_size.to_be_bytes();
    }
}
unsafe impl Sliceable for UdpPacket {}
