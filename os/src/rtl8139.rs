extern crate alloc;

use crate::arch::x86_64::busy_loop_hint;
use crate::arch::x86_64::read_io_port_u32;
use crate::arch::x86_64::read_io_port_u8;
use crate::arch::x86_64::write_io_port_u16;
use crate::arch::x86_64::write_io_port_u32;
use crate::arch::x86_64::write_io_port_u8;
use crate::error::Result;
use crate::executor::Task;
use crate::executor::TimeoutFuture;
use crate::executor::ROOT_EXECUTOR;
use crate::mutex::Mutex;
use crate::network::ArpPacket;
use crate::network::EthernetAddr;
use crate::network::Network;
use crate::network::NetworkInterface;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::println;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use core::mem::size_of;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::slice;

pub struct Rtl8139Driver {}
impl Rtl8139Driver {
    pub fn default() -> Self {
        Rtl8139Driver {}
    }
}
impl PciDeviceDriver for Rtl8139Driver {
    fn supports(&self, vp: VendorDeviceId) -> bool {
        const RTL8139_ID: VendorDeviceId = VendorDeviceId {
            vendor: 0x10ec,
            device: 0x8139,
        };
        vp == RTL8139_ID
    }
    fn attach(&self, bdf: BusDeviceFunction) -> Result<Box<dyn PciDeviceDriverInstance>> {
        Ok(Box::new(Rtl8139DriverInstance::new(bdf)?) as Box<dyn PciDeviceDriverInstance>)
    }
    fn name(&self) -> &str {
        "Rtl8139Driver"
    }
}

// RTL8139_RXBUF_SIZE == 8192+16+1500 for WRAP=1
// See https://wiki.osdev.org/RTL8139 for more info
const RTL8139_RXBUF_SIZE: usize = 9708;
struct RxContext {
    buf: Pin<Box<[u8; RTL8139_RXBUF_SIZE]>>,
    next_index: usize,
    packet_count: usize,
}
impl RxContext {
    fn new() -> Self {
        Self {
            buf: Box::pin(unsafe { MaybeUninit::zeroed().assume_init() }),
            next_index: 0,
            packet_count: 0,
        }
    }
}

struct TxContext {
    pending_packets: VecDeque<Box<[u8]>>,
    queued_packets: [Option<Pin<Box<[u8]>>>; 4],
    next_index: usize,
    packet_count: usize,
}
impl TxContext {
    fn new() -> Self {
        const PACKET_NONE: Option<Pin<Box<[u8]>>> = None;
        Self {
            pending_packets: VecDeque::new(),
            queued_packets: [PACKET_NONE; 4],
            next_index: 0,
            packet_count: 0,
        }
    }
}

struct Rtl8139 {
    _bdf: BusDeviceFunction,
    io_base: u16,
    eth_addr: EthernetAddr,
    rx: Mutex<RxContext>,
    tx: Mutex<TxContext>,
}
impl Rtl8139 {
    // https://wiki.osdev.org/RTL8139
    fn new(bdf: BusDeviceFunction) -> Result<Self> {
        let pci = Pci::take();
        pci.disable_interrupt(bdf)?;
        pci.enable_bus_master(bdf)?;
        // Assume that BAR0 has IO Port address
        let io_base = pci.try_bar0_io(bdf)?;
        let mut eth_addr = [0u8; 6];
        for (i, e) in eth_addr.iter_mut().enumerate() {
            *e = read_io_port_u8(io_base + i as u16);
        }
        let eth_addr = EthernetAddr::new(eth_addr);
        println!("Rtl8139: eth_addr: {:?}", eth_addr);
        // Turn on
        write_io_port_u8(io_base + 0x52, 0);
        // Software Reset
        write_io_port_u8(io_base + 0x37, 0x10);
        while (read_io_port_u8(io_base + 0x37) & 0x10) != 0 {
            busy_loop_hint();
        }

        let d = Self {
            _bdf: bdf,
            io_base,
            eth_addr,
            rx: Mutex::new(RxContext::new()),
            tx: Mutex::new(TxContext::new()),
        };
        let rx_buf_ptr = {
            let rx = d.rx.lock();
            rx.buf.as_ref().as_ptr()
        };
        assert!((rx_buf_ptr as usize) < ((u32::MAX) as usize - RTL8139_RXBUF_SIZE));
        write_io_port_u32(io_base + 0x30, rx_buf_ptr as usize as u32);
        write_io_port_u32(io_base + 0x3C, 0x0005); // Interrupts: Transmit OK, Receive OK
        write_io_port_u32(io_base + 0x44, 0x8f); // WRAP+AB+AM+APM+AAP (receive any type of packets)
        write_io_port_u8(io_base + 0x37, 0x0C); // RE+TE (Enable Rx and Tx)

        Ok(d)
    }
    fn queue_packet(&self, packet: Box<[u8]>) -> Result<()> {
        let mut tx = self.tx.lock();
        tx.pending_packets.push_back(packet);
        Ok(())
    }
    fn update_rx_buf_read_ptr(&self, read_index: u16) {
        // CAPR: Current Address of Packet Read
        write_io_port_u16(self.io_base + 0x38, read_index);
    }
    fn poll_tx(&self) -> Result<()> {
        let mut tx = self.tx.lock();
        loop {
            // Fill Tx Queue as much as possible
            let index = tx.next_index;
            let tx_cmd = read_io_port_u32(self.io_base + (0x10 + 4 * index) as u16);
            // bit 0-12
            // - packet size. 1792 is the max size
            // bit 13 (OWND):
            // - 0 if the data is owned by the device (DMA is in progress)
            // - 1 if the data is owned by software (DMA is completed)
            // - setting this bit to 0 will clear all other bits as well
            // bit 15 (TXOK):
            // - 0 if the data is being sent to network (transmission is in progress)
            // - 1 if the data is transmitted (transmission is completed)

            // (tx_queued_packet, OWND, TXOK)
            // Init > (None, 1, 0)
            // Write buffer addr > (Some(_), 0, 0)
            // DMA completed > (Some(_), 1, 0)
            // Transmission completed > (Some(_), 1, 1)
            // Queued packet discarded > (None, _, _)

            if tx.queued_packets[index].is_some() {
                if tx_cmd & (1 << 13) != 0 && tx_cmd & (1 << 15) != 0 {
                    // Tx operation is done, release the entry
                    tx.queued_packets[index] = None;
                } else {
                    // No more Tx entry are available.
                    break;
                }
            }
            // qp is None == we can send a packet
            if let Some(next_packet) = tx.pending_packets.pop_front() {
                // Enqueue Tx packet
                let next_packet = Pin::new(next_packet);
                let packet_len: u32 = next_packet.as_ref().len().try_into()?;
                write_io_port_u32(
                    self.io_base + 0x20 + 4 * index as u16, /* TSAD[self.next_tx_reg_idx], buf addr */
                    (next_packet.as_ref().get_ref().as_ptr() as usize).try_into()?,
                );
                core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
                write_io_port_u32(
                    self.io_base + 0x10 + 4 * index as u16, /* TSD[self.next_tx_reg_idx], size + flags */
                    packet_len,
                );
                tx.queued_packets[index] = Some(next_packet);
                tx.next_index = (index + 1) % 4;
                tx.packet_count += 1;
            } else {
                // No more packets to send
                break;
            }
        }
        Ok(())
    }
    fn poll_rx(&self) -> Result<()> {
        let mut rx = self.rx.lock();
        loop {
            let rx_buf_ptr = unsafe { rx.buf.as_mut().get_unchecked_mut().as_mut_ptr() };
            let rx_desc_ptr = unsafe { rx_buf_ptr.add(rx.next_index) };
            let rx_status = unsafe { *(rx_desc_ptr.add(0) as *const u16) };
            if rx_status == 0 {
                break;
            }
            // Rx Status Info
            // bit 0
            // - 1 if a good packet is received
            // bit 1-5
            // - Other than 0 means error
            // bit 1
            // - 1 if broardcast packet is received.
            // bit 2
            // - 1 if physical address is received.
            let packet_len = unsafe { *(rx_desc_ptr.offset(2) as *const u16) } as usize;
            let arp = unsafe { slice::from_raw_parts(rx_desc_ptr.offset(4), packet_len) };
            if packet_len >= size_of::<ArpPacket>() {
                let arp = ArpPacket::from_bytes(&arp[0..size_of::<ArpPacket>()]);
            }

            // Erase the packet for the next cycle
            unsafe { rx_desc_ptr.write_bytes(0, packet_len + 4) };
            self.update_rx_buf_read_ptr(rx.next_index.try_into().unwrap());
            write_io_port_u16(self.io_base + 0x3E, 0x1);
            rx.next_index += packet_len + 4;
            if rx.next_index >= 8192 {
                rx.next_index %= 8192;
                self.update_rx_buf_read_ptr(rx.next_index.try_into().unwrap());
                write_io_port_u16(self.io_base + 0x3E, 0x11 /*RxOverflow + RxOk*/);
            }
            rx.packet_count += 1;
        }
        Ok(())
    }
    async fn poll(&self) -> Result<()> {
        self.poll_tx()?;
        self.poll_rx()?;
        TimeoutFuture::new_ms(100).await;
        Ok(())
    }
}
impl NetworkInterface for Rtl8139 {
    fn name(&self) -> &str {
        "RTL8139"
    }
    fn ethernet_addr(&self) -> EthernetAddr {
        self.eth_addr
    }
    fn queue_packet(&self, packet: Box<[u8]>) -> Result<()> {
        self.queue_packet(packet)
    }
}

pub struct Rtl8139DriverInstance {}
impl Rtl8139DriverInstance {
    fn new(bdf: BusDeviceFunction) -> Result<Self> {
        let d = Rc::new(Rtl8139::new(bdf)?);
        {
            let d = Rc::downgrade(&d);
            Network::take().register_interface(d);
        }
        (*ROOT_EXECUTOR.lock()).spawn(Task::new(async move {
            loop {
                d.poll().await?
            }
        }));
        Ok(Self {})
    }
}
impl PciDeviceDriverInstance for Rtl8139DriverInstance {
    fn name(&self) -> &str {
        "Rtl8139DriverInstance"
    }
}
