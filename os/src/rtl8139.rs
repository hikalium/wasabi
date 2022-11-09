extern crate alloc;

use crate::allocator::ALLOCATOR;
use crate::arch::x86_64::busy_loop_hint;
use crate::arch::x86_64::read_io_port_u16;
use crate::arch::x86_64::read_io_port_u8;
use crate::arch::x86_64::write_io_port_u16;
use crate::arch::x86_64::write_io_port_u32;
use crate::arch::x86_64::write_io_port_u8;
use crate::error::Result;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::println;
use alloc::boxed::Box;
use alloc::fmt;
use alloc::fmt::Debug;
use core::alloc::Layout;
use core::marker::PhantomPinned;
use core::mem::size_of;
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

#[repr(packed)]
#[derive(Clone, Copy)]
struct EthernetAddress {
    mac: [u8; 6],
}
impl EthernetAddress {
    fn new(mac: [u8; 6]) -> Self {
        Self { mac }
    }
    const fn broardcast() -> Self {
        Self {
            mac: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        }
    }
    const fn zero() -> Self {
        Self {
            mac: [0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        }
    }
}
impl Debug for EthernetAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5],
        )
    }
}
#[repr(packed)]
struct IpV4Addr {
    ip: [u8; 4],
}
impl IpV4Addr {
    fn new(ip: [u8; 4]) -> Self {
        Self { ip }
    }
}
impl Debug for IpV4Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.ip[0], self.ip[1], self.ip[2], self.ip[3],
        )
    }
}
#[repr(packed)]
#[allow(unused)]
struct EthernetHeader {
    dst_eth: EthernetAddress,
    src_eth: EthernetAddress,
    eth_type: [u8; 2],
}
#[repr(packed)]
#[allow(unused)]
struct ArpPacket {
    eth_header: EthernetHeader,
    hw_type: [u8; 2],
    proto_type: [u8; 2],
    hw_addr_size: u8,
    proto_addr_size: u8,
    op: [u8; 2],
    sender_mac: EthernetAddress,
    sender_ip: IpV4Addr,
    target_mac: EthernetAddress,
    target_ip: IpV4Addr,
    //
    _pinned: PhantomPinned,
}
impl ArpPacket {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // SAFETY: any bytes can be parsed as the ARP packet
        if bytes.len() >= size_of::<ArpPacket>() {
            let mut tmp = [0u8; size_of::<ArpPacket>()];
            tmp.copy_from_slice(&bytes[0..size_of::<ArpPacket>()]);
            Ok(unsafe { core::mem::transmute(tmp) })
        } else {
            Err(crate::error::Error::Failed("too short"))
        }
    }
    fn request(src_eth: EthernetAddress, src_ip: IpV4Addr, dst_ip: IpV4Addr) -> Self {
        Self {
            eth_header: EthernetHeader {
                dst_eth: src_eth,
                src_eth: EthernetAddress::broardcast(),
                eth_type: [0x08, 0x06],
            },
            hw_type: [0x00, 0x01],
            proto_type: [0x08, 0x00],
            hw_addr_size: 6,
            proto_addr_size: 4,
            op: [0x00, 0x01],
            sender_mac: src_eth,
            sender_ip: src_ip,
            target_mac: EthernetAddress::zero(), // target_mac = Unknown
            target_ip: dst_ip,
            //
            _pinned: PhantomPinned,
        }
    }
}
impl Debug for ArpPacket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ArpPacket {{ op: {}, sender: {:?} ({:?}), target: {:?} ({:?}) }}",
            match (self.op[0], self.op[1]) {
                (0, 1) => "request",
                (0, 2) => "response",
                (_, _) => "unknown",
            },
            self.sender_ip,
            self.sender_mac,
            self.target_ip,
            self.target_mac,
        )
    }
}
const _: () = assert!(size_of::<ArpPacket>() == 42);

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
