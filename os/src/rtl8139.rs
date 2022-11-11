extern crate alloc;

use crate::allocator::ALLOCATOR;
use crate::arch::x86_64::busy_loop_hint;
use crate::arch::x86_64::read_io_port_u16;
use crate::arch::x86_64::read_io_port_u8;
use crate::arch::x86_64::write_io_port_u16;
use crate::arch::x86_64::write_io_port_u32;
use crate::arch::x86_64::write_io_port_u8;
use crate::error::Result;
use crate::network::ArpPacket;
use crate::network::EthernetAddress;
use crate::network::IpV4Addr;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::println;
use alloc::boxed::Box;
use core::alloc::Layout;
use core::mem::size_of;
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

const RTL8139_RXBUF_SIZE: usize = 8208;
pub struct Rtl8139DriverInstance<'a> {
    #[allow(dead_code)]
    bdf: BusDeviceFunction,
    #[allow(dead_code)]
    rx_buffer: &'a mut [u8; RTL8139_RXBUF_SIZE],
}
impl<'a> Rtl8139DriverInstance<'a> {
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
        let eth_addr = EthernetAddress::new(eth_addr);
        println!("eth_addr: {:?}", eth_addr);
        // Turn on
        write_io_port_u8(io_base + 0x52, 0);
        // Software Reset
        write_io_port_u8(io_base + 0x37, 0x10);
        while (read_io_port_u8(io_base + 0x37) & 0x10) != 0 {
            busy_loop_hint();
        }
        println!("Software Reset Done!");
        let rx_buffer = unsafe {
            &mut *(ALLOCATOR.alloc_with_options(
                Layout::from_size_align(RTL8139_RXBUF_SIZE, 1).expect("Invalid Layout"),
            ) as *mut [u8; RTL8139_RXBUF_SIZE])
        };
        assert!((rx_buffer.as_ptr() as usize) < ((u32::MAX) as usize - RTL8139_RXBUF_SIZE));
        println!("rx_buffer is at {:#p}", rx_buffer);

        write_io_port_u32(io_base + 0x30, rx_buffer.as_ptr() as usize as u32);

        write_io_port_u32(io_base + 0x3C, 0x0005); // Interrupts: Transmit OK, Receive OK

        write_io_port_u16(io_base + 0x44, 0xf); // AB+AM+APM+AAP

        write_io_port_u8(io_base + 0x37, 0x0C); // RE+TE

        let arp_req = Box::pin(ArpPacket::request(
            eth_addr,
            IpV4Addr::new([10, 0, 2, 15]),
            IpV4Addr::new([10, 0, 2, 2]),
        ));

        write_io_port_u32(
            io_base + 0x20, /* TSAD[0] */
            (arp_req.as_ref().get_ref() as *const ArpPacket as usize).try_into()?,
        );
        write_io_port_u32(
            io_base + 0x10, /* TSD[0] */
            size_of::<ArpPacket>().try_into()?,
        );
        println!("Sending ARP Request...");
        println!("{arp_req:?}");
        let write_count = loop {
            let write_count = read_io_port_u16(io_base + 0x3A);
            if write_count != 0 {
                break write_count;
            }
            busy_loop_hint();
        };
        println!("write_count: {}", write_count);
        let rx_status = unsafe { *(rx_buffer.as_ptr() as *const u16) };
        println!("rx_status: {}", rx_status);
        let packet_len = unsafe { *(rx_buffer.as_ptr().offset(2) as *const u16) } as usize;
        println!("packet_len: {}", packet_len);
        println!("Received packet:");
        let arp = unsafe { slice::from_raw_parts(rx_buffer.as_ptr().offset(4), packet_len) };
        if packet_len >= size_of::<ArpPacket>() {
            let arp = ArpPacket::from_bytes(&arp[0..size_of::<ArpPacket>()]);
            println!("{arp:?}");
        }

        Ok(Rtl8139DriverInstance { bdf, rx_buffer })
    }
}
impl<'a> PciDeviceDriverInstance for Rtl8139DriverInstance<'a> {
    fn name(&self) -> &str {
        "Rtl8139DriverInstance"
    }
}
