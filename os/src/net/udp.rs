extern crate alloc;

use crate::error::Result;
use crate::info;
use crate::mutex::Mutex;
use crate::net::checksum::InternetChecksum;
use crate::net::ip::IpV4Packet;
use alloc::collections::VecDeque;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::vec::Vec;
use core::future::Future;
use core::marker::PhantomPinned;
use core::pin::Pin;
use core::task::Context;
use core::task::Poll;
use noli::mem::Sliceable;

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

pub struct UdpSocket {
    tx_queue: Mutex<VecDeque<Vec<u8>>>,
    rx_queue: Mutex<VecDeque<Vec<u8>>>,
}
impl Default for UdpSocket {
    fn default() -> Self {
        Self {
            tx_queue: Mutex::new(VecDeque::new()),
            rx_queue: Mutex::new(VecDeque::new()),
        }
    }
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
        self.rx_queue.lock().push_back(in_packet);
        Ok(())
    }
    pub fn push_tx_packet(&self, packet: Vec<u8>) -> Result<()> {
        info!("net: udp: push_tx_packet");
        self.tx_queue.lock().push_back(packet);
        Ok(())
    }
    pub fn recv(&self) -> UdpSocketRecvFuture {
        UdpSocketRecvFuture {
            rx_queue: &self.rx_queue,
            _pinned: PhantomPinned,
        }
    }
}

pub struct UdpSocketRecvFuture<'a> {
    rx_queue: &'a Mutex<VecDeque<Vec<u8>>>,
    _pinned: PhantomPinned,
}
impl<'a> Future for UdpSocketRecvFuture<'a> {
    type Output = Vec<u8>;
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Vec<u8>> {
        let mut_self = unsafe { self.get_unchecked_mut() };
        let packet = mut_self.rx_queue.lock().pop_front();
        if let Some(packet) = packet {
            Poll::Ready(packet)
        } else {
            Poll::Pending
        }
    }
}
