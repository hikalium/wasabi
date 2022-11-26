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
use crate::network::ArpPacket;
use crate::network::EthernetAddr;
use crate::network::IpV4Addr;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::println;
use crate::util::Sliceable;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
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
        // akemi
        // 8086:02ed Comet Lake PCH-LP USB 3.1 xHCI Host Controller
        // 8086:02f0 Comet Lake PCH-LP CNVi WiFi
        // 8086:02e8 Serial IO I2C Host Controller
        // 8086:02e9 Comet Lake Serial IO I2C Host Controller
        // 8086:02c5 Comet Lake Serial IO I2C Host Controller
        // 8086:02c8 Comet Lake PCH-LP cAVS
        // 8086:02a3 Comet Lake PCH-LP SMBus Host Controller
        // 8086:02a4 Comet Lake SPI (flash) Controller

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
struct Rtl8139 {
    _bdf: BusDeviceFunction,
    io_base: u16,
    eth_addr: EthernetAddr,
    //
    rx_buf: Pin<Box<[u8; RTL8139_RXBUF_SIZE]>>,
    next_rx_byte_idx: usize,
    //
    tx_pending_packets: VecDeque<Box<[u8]>>,
    tx_queued_packets: [Option<Pin<Box<[u8]>>>; 4],
    next_tx_reg_idx: usize,
}
impl Rtl8139 {
    // https://wiki.osdev.org/RTL8139
    fn new(bdf: BusDeviceFunction) -> Result<Self> {
        let pci = Pci::take();
        pci.disable_interrupt(bdf)?;
        pci.enable_bus_master(bdf)?;
        if let Some(caps) = pci.capabilities(bdf) {
            for cap in caps {
                println!("CAP {:#04X}", cap.id);
            }
        }
        // Assume that BAR0 has IO Port address
        let io_base = pci.try_bar0_io(bdf)?;
        let mut eth_addr = [0u8; 6];
        for (i, e) in eth_addr.iter_mut().enumerate() {
            *e = read_io_port_u8(io_base + i as u16);
        }
        let eth_addr = EthernetAddr::new(eth_addr);
        println!("eth_addr: {:?}", eth_addr);
        // Turn on
        write_io_port_u8(io_base + 0x52, 0);
        // Software Reset
        write_io_port_u8(io_base + 0x37, 0x10);
        while (read_io_port_u8(io_base + 0x37) & 0x10) != 0 {
            busy_loop_hint();
        }
        println!("Software Reset Done!");

        const PACKET_NONE: Option<Pin<Box<[u8]>>> = None;
        let d = Self {
            _bdf: bdf,
            io_base,
            eth_addr,
            //
            rx_buf: Box::pin(unsafe { MaybeUninit::zeroed().assume_init() }),
            next_rx_byte_idx: 0,
            //
            tx_pending_packets: VecDeque::new(),
            tx_queued_packets: [PACKET_NONE; 4],
            next_tx_reg_idx: 0,
        };
        let rx_buf_ptr = d.rx_buf.as_ref().as_ptr();
        assert!((rx_buf_ptr as usize) < ((u32::MAX) as usize - RTL8139_RXBUF_SIZE));
        println!("rx_buffer is at {:#p}", rx_buf_ptr);
        write_io_port_u32(io_base + 0x30, rx_buf_ptr as usize as u32);
        write_io_port_u32(io_base + 0x3C, 0x0005); // Interrupts: Transmit OK, Receive OK
        write_io_port_u16(io_base + 0x44, 0x8f); // WRAP+AB+AM+APM+AAP (receive any type of packets)
        write_io_port_u8(io_base + 0x37, 0x0C); // RE+TE (Enable Rx and Tx)

        Ok(d)
    }
    fn queue_packet(&mut self, packet: Box<[u8]>) -> Result<()> {
        self.tx_pending_packets.push_back(packet);
        Ok(())
    }
    async fn poll(&mut self) -> Result<()> {
        // Tx operations
        loop {
            // Fill Tx Queue as much as possible
            let tx_cmd = read_io_port_u32(self.io_base + (0x20 + 4 * self.next_tx_reg_idx) as u16);
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

            let qp = &mut self.tx_queued_packets[self.next_tx_reg_idx];
            if qp.is_some() && tx_cmd & (1 << 13) != 0 && tx_cmd & (1 << 15) != 0 {
                // Tx operation is done, release the entry
                qp.take();
                println!("Tx[{}] done!", self.next_tx_reg_idx);
            }

            let next_packet = self.tx_pending_packets.pop_front();
            if let (None, Some(next_packet)) = (&qp, next_packet) {
                // Enqueue Tx packet
                let next_packet = Pin::new(next_packet);
                write_io_port_u32(
                    self.io_base + 0x20 + 4 * self.next_tx_reg_idx as u16, /* TSAD[self.next_tx_reg_idx], buf addr */
                    (next_packet.as_ref().get_ref().as_ptr() as usize).try_into()?,
                );
                write_io_port_u32(
                    self.io_base + 0x10 + 4 * self.next_tx_reg_idx as u16, /* TSD[self.next_tx_reg_idx], size + flags */
                    next_packet.as_ref().len().try_into()?,
                );
                println!("Tx[{}] queued!", self.next_tx_reg_idx);
                *qp = Some(next_packet);
                self.next_tx_reg_idx = (self.next_tx_reg_idx + 1) % 4;
            } else {
                // No more Tx entry are available.
                break;
            }
        }

        // Rx Operations
        loop {
            let rx_buf_ptr = self.rx_buf.as_ref().as_ptr();
            let rx_desc_ptr = unsafe { rx_buf_ptr.add(self.next_rx_byte_idx) };
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
            println!("rx_status: {}", rx_status);
            let packet_len = unsafe { *(rx_desc_ptr.offset(2) as *const u16) } as usize;
            println!("packet_len: {}", packet_len);
            println!("Received packet:");
            let arp = unsafe { slice::from_raw_parts(rx_desc_ptr.offset(4), packet_len) };
            if packet_len >= size_of::<ArpPacket>() {
                let arp = ArpPacket::from_bytes(&arp[0..size_of::<ArpPacket>()]);
                println!("{arp:?}");
            }
            self.next_rx_byte_idx += packet_len + 4;
        }

        TimeoutFuture::new_ms(100).await;
        Ok(())
    }
}

pub struct Rtl8139DriverInstance {}
impl Rtl8139DriverInstance {
    fn new(bdf: BusDeviceFunction) -> Result<Self> {
        let mut d = Rtl8139::new(bdf)?;
        (*ROOT_EXECUTOR.lock()).spawn(Task::new(async move {
            let arp_req = Box::pin(ArpPacket::request(
                d.eth_addr,
                IpV4Addr::new([10, 0, 2, 15]),
                IpV4Addr::new([10, 0, 2, 2]),
            ));
            d.queue_packet(arp_req.copy_into_slice())?;
            d.queue_packet(arp_req.copy_into_slice())?;

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
