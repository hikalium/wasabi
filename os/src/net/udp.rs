extern crate alloc;

use crate::error::Result;
use crate::info;
use crate::net::checksum::InternetChecksum;
use crate::net::ip::IpV4Packet;
use crate::util::Sliceable;
use alloc::collections::VecDeque;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::vec::Vec;

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
    pub fn set_data_size(&mut self, data_size: usize) -> Result<()> {
        self.data_size = u16::try_from(data_size)?.to_be_bytes();
        Ok(())
    }
    pub fn data_size(&self) -> usize {
        u16::from_be_bytes(self.data_size) as usize
    }
}
unsafe impl Sliceable for UdpPacket {}
impl Debug for UdpPacket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UDP :{} -> :{}", self.src_port(), self.dst_port(),)
    }
}

#[derive(Default)]
pub struct UdpSocket {
    tx_queue: VecDeque<Vec<u8>>,
    rx_queue: VecDeque<Vec<u8>>,
}
impl Debug for UdpSocket {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "UdpSocket{{ }}")
    }
}
impl UdpSocket {
    pub fn handle_rx(&self, in_bytes: &[u8]) -> Result<()> {
        let in_packet = Vec::from(in_bytes);
        let in_udp = UdpPacket::from_slice(&in_packet)?;
        info!("net: udp: recv: {in_udp:?}",);
        Ok(())
    }
    pub fn push_tx_packet(&mut self, packet: Vec<u8>) -> Result<()> {
        info!("net: udp: push_tx_packet");
        self.tx_queue.push_back(packet);
        Ok(())
    }
}
