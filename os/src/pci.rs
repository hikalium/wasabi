extern crate alloc;

use crate::acpi::Mcfg;
use crate::allocator::ALLOCATOR;
use crate::error::Result;
use crate::error::WasabiError;
use crate::println;
use crate::x86::busy_loop_hint;
use crate::x86::read_io_port;
use crate::x86::write_io_port_u16;
use crate::x86::write_io_port_u32;
use crate::x86::write_io_port_u8;
use alloc::boxed::Box;
use alloc::collections::btree_map::BTreeMap;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::cell::RefCell;
use core::fmt;
use core::ops::Range;

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct VendorDeviceId {
    pub vendor: u16,
    pub device: u16,
}
impl VendorDeviceId {
    pub fn fmt_common(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(vendor: {:#06X}, device: {:#06X})",
            self.vendor, self.device,
        )
    }
}
impl fmt::Debug for VendorDeviceId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.fmt_common(f)
    }
}
impl fmt::Display for VendorDeviceId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.fmt_common(f)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BusDeviceFunction {
    id: u16,
}
const MASK_BUS: usize = 0b1111_1111_0000_0000;
const SHIFT_BUS: usize = 8;
const MASK_DEVICE: usize = 0b0000_0000_1111_1000;
const SHIFT_DEVICE: usize = 3;
const MASK_FUNCTION: usize = 0b0000_0000_0000_0111;
const SHIFT_FUNCTION: usize = 0;
impl BusDeviceFunction {
    pub fn new(bus: usize, device: usize, function: usize) -> Result<Self> {
        if !(0..256).contains(&bus) || !(0..32).contains(&device) || !(0..8).contains(&function) {
            Err(WasabiError::PciBusDeviceFunctionOutOfRange)
        } else {
            Ok(Self {
                id: ((bus << SHIFT_BUS) | (device << SHIFT_DEVICE) | (function << SHIFT_FUNCTION))
                    as u16,
            })
        }
    }
    pub fn bus(&self) -> usize {
        ((self.id as usize) & MASK_BUS) >> SHIFT_BUS
    }
    pub fn device(&self) -> usize {
        ((self.id as usize) & MASK_DEVICE) >> SHIFT_DEVICE
    }
    pub fn function(&self) -> usize {
        ((self.id as usize) & MASK_FUNCTION) >> SHIFT_FUNCTION
    }
    pub fn iter() -> BusDeviceFunctionIterator {
        BusDeviceFunctionIterator { next_id: 0 }
    }
    pub fn fmt_common(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(bus: {:#04X}, device: {:#04X}, function: {:#03X})",
            self.bus(),
            self.device(),
            self.function()
        )
    }
}
impl fmt::Debug for BusDeviceFunction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.fmt_common(f)
    }
}
impl fmt::Display for BusDeviceFunction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.fmt_common(f)
    }
}
pub struct BusDeviceFunctionIterator {
    next_id: usize,
}
impl Iterator for BusDeviceFunctionIterator {
    type Item = BusDeviceFunction;
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next_id;
        if id > 0xffff {
            None
        } else {
            self.next_id += 1;
            let id = id as u16;
            Some(BusDeviceFunction { id })
        }
    }
}

pub struct EthernetAddress {
    mac: [u8; 6],
}
impl EthernetAddress {
    fn new(mac: &[u8; 6]) -> Self {
        EthernetAddress { mac: *mac }
    }
}
impl fmt::Display for EthernetAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:",
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5],
        )
    }
}

pub struct Rtl8139Driver {}
impl Rtl8139Driver {
    fn new() -> Self {
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
        Ok(Box::new(Rtl8139DriverInstance::new(bdf)) as Box<dyn PciDeviceDriverInstance>)
    }
    fn name(&self) -> &str {
        "Rtl8139Driver"
    }
}
const RTL8139_RXBUF_SIZE: usize = 8208;
const ARP_REQ_SAMPLE_DATA: [u8; 42] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // dst eth addr
    0x52, 0x54, 0x00, 0x12, 0x34, 0x57, // src eth addr
    0x08, 0x06, // eth_type = ARP
    0x00, 0x01, // hw_type = Ethernet
    0x08, 0x00, // proto_type = IPv4
    0x06, // hw_addr_size = 6 bytes
    0x04, // proto_addr_type = 4 bytes
    0x00, 0x01, // operation = ARP request
    0x52, 0x54, 0x00, 0x12, 0x34, 0x57, // sender_mac = 52:54:00:12:34:57
    0x0A, 0x00, 0x02, 0x0F, // sender_ip = 10.0.2.15
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // target_mac = Unknown
    0x0A, 0x00, 0x02, 0x02, // target_ip = 10.0.2.2
];
pub struct Rtl8139DriverInstance<'a> {
    #[allow(dead_code)]
    bdf: BusDeviceFunction,
    #[allow(dead_code)]
    rx_buffer: &'a mut [u8; RTL8139_RXBUF_SIZE],
}
impl<'a> Rtl8139DriverInstance<'a> {
    // https://wiki.osdev.org/RTL8139
    fn new(bdf: BusDeviceFunction) -> Self {
        let pci = Pci::take();
        println!("Instantiating RTL8139 NIC @ {}...", bdf);
        let cmd_and_status = pci.read_register_u32(bdf, 0x04);
        pci.write_register_u32(
            bdf,
            0x04, /* Command and status */
            (1 << 2)/* Bus Master Enable */ | (1 << 10) /* Interrupt Disable */ | cmd_and_status,
        );
        // Assume that BAR0 has IO Port address
        let bar0 = pci.read_register_u32(bdf, 0x10);
        assert_eq!(bar0 & 1, 1 /* I/O space */);
        let io_base = (bar0 ^ 1) as u16;
        let mut eth_addr = [0u8; 6];
        for (i, e) in eth_addr.iter_mut().enumerate() {
            *e = read_io_port(io_base + i as u16);
        }
        let eth_addr = EthernetAddress::new(&eth_addr);
        println!("eth_addr: {}", eth_addr);
        // Turn on
        write_io_port_u8(io_base + 0x52, 0);
        // Software Reset
        write_io_port_u8(io_base + 0x37, 0x10);
        while (read_io_port(io_base + 0x37) & 0x10) != 0 {
            busy_loop_hint();
        }
        println!("Software Reset Done!");
        let rx_buffer = unsafe {
            &mut *(ALLOCATOR.alloc_with_options(
                Layout::from_size_align(RTL8139_RXBUF_SIZE, 1).expect("Invalid Layout"),
            ) as *mut [u8; RTL8139_RXBUF_SIZE])
        };
        assert!(
            (rx_buffer.as_ptr() as usize)
                < ((core::primitive::u32::MAX) as usize - RTL8139_RXBUF_SIZE)
        );
        println!("rx_buffer is at {:#p}", rx_buffer);

        write_io_port_u32(io_base + 0x30, rx_buffer.as_ptr() as usize as u32);

        write_io_port_u32(io_base + 0x3C, 0x0005); // Interrupts: Transmit OK, Receive OK

        write_io_port_u16(io_base + 0x44, 0xf); // AB+AM+APM+AAP

        write_io_port_u8(io_base + 0x37, 0x0C); // RE+TE

        write_io_port_u32(
            io_base + 0x20, /* TSAD[0] */
            ARP_REQ_SAMPLE_DATA.as_ptr() as usize as u32,
        );
        write_io_port_u32(
            io_base + 0x10, /* TSD[0] */
            ARP_REQ_SAMPLE_DATA.len() as u32,
        );

        Rtl8139DriverInstance { bdf, rx_buffer }
    }
}
impl<'a> PciDeviceDriverInstance for Rtl8139DriverInstance<'a> {
    fn name(&self) -> &str {
        "Rtl8139DriverInstance"
    }
}

pub trait PciDeviceDriver {
    fn supports(&self, vp: VendorDeviceId) -> bool;
    fn attach(&self, bdf: BusDeviceFunction) -> Result<Box<dyn PciDeviceDriverInstance>>;
    fn name(&self) -> &str;
}
pub trait PciDeviceDriverInstance {
    fn name(&self) -> &str;
}

impl fmt::Debug for dyn PciDeviceDriver {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PciDeviceDriver{{ name: {} }}", self.name())
    }
}

pub struct Pci {
    ecm_range: Range<usize>,
    drivers: Vec<Rc<Box<dyn PciDeviceDriver>>>,
    devices: RefCell<BTreeMap<BusDeviceFunction, Rc<Box<dyn PciDeviceDriver>>>>,
}
impl Pci {
    pub fn new(mcfg: &Mcfg) -> Self {
        println!("{:?}", mcfg);
        for i in 0..mcfg.num_of_entries() {
            let e = mcfg.entry(i).expect("Out of range");
            println!("{}", e);
        }
        // To simplify, assume that there is one mcfg entry that maps all the pci configuration spaces.
        assert!(mcfg.num_of_entries() == 1);
        let pci_config_space_base = mcfg.entry(0).expect("Out of range").base_address() as usize;
        let pci_config_space_end = pci_config_space_base + (1 << 24);
        println!(
            "PCI config space is mapped at: [{:#018X},{:#018X})",
            pci_config_space_base, pci_config_space_end
        );

        let drivers = vec![Rc::new(
            Box::new(Rtl8139Driver::new()) as Box<dyn PciDeviceDriver>
        )];

        Pci {
            ecm_range: pci_config_space_base..pci_config_space_end,
            drivers,
            devices: RefCell::new(BTreeMap::new()),
        }
    }
    fn ecm_base<T>(&self, id: BusDeviceFunction) -> *mut T {
        (self.ecm_range.start + ((id.id as usize) << 12)) as *mut T
    }
    pub fn read_register_u8(&self, id: BusDeviceFunction, byte_offset: usize) -> u8 {
        assert!((0..256).contains(&byte_offset));
        unsafe { *self.ecm_base::<u8>(id).add(byte_offset) }
    }
    pub fn read_register_u16(&self, id: BusDeviceFunction, byte_offset: usize) -> u16 {
        assert!((0..256).contains(&byte_offset));
        assert!(byte_offset & 1 == 0);
        unsafe { *self.ecm_base::<u16>(id).add(byte_offset >> 1) }
    }
    pub fn read_register_u32(&self, id: BusDeviceFunction, byte_offset: usize) -> u32 {
        assert!((0..256).contains(&byte_offset));
        assert!(byte_offset & 3 == 0);
        unsafe { *self.ecm_base::<u32>(id).add(byte_offset >> 2) }
    }
    pub fn write_register_u32(&self, id: BusDeviceFunction, byte_offset: usize, data: u32) {
        assert!((0..256).contains(&byte_offset));
        assert!(byte_offset & 3 == 0);
        unsafe {
            *self.ecm_base::<u32>(id).add(byte_offset >> 2) = data;
        }
    }
    pub fn read_vendor_id_and_device_id(&self, id: BusDeviceFunction) -> Option<VendorDeviceId> {
        let vendor = self.read_register_u16(id, 0);
        let device = self.read_register_u16(id, 2);
        if vendor == 0xFFFF || device == 0xFFFF {
            // Not connected
            None
        } else {
            Some(VendorDeviceId { vendor, device })
        }
    }
    pub fn probe_devices(&self) {
        println!("Probing PCI devices...");
        for bdf in BusDeviceFunction::iter() {
            if let Some(vd) = self.read_vendor_id_and_device_id(bdf) {
                println!("{:?}: {:?}", bdf, vd);
                let header_type = self.read_register_u8(bdf, 0x0e);
                println!("  header_type: {:#02X}", header_type);
                if header_type != 0 {
                    // Support only header_type == 0 for now
                    continue;
                }
                for i in 0..6 {
                    let bar = self.read_register_u32(bdf, 0x10 + i * 4);
                    if bar == 0 {
                        continue;
                    }
                    println!("  BAR{}: {:#010X}", i, bar);
                }
                if self.devices.borrow_mut().contains_key(&bdf) {
                    continue;
                }
                for d in &self.drivers {
                    if d.supports(vd) && d.attach(bdf).is_ok() {
                        self.devices.borrow_mut().insert(bdf, d.clone());
                    }
                }
            }
        }
    }
    pub fn list_drivers(&self) {
        println!("{:?}", self.drivers)
    }
    pub fn list_devices(&self) {
        println!("{:?}", self.devices)
    }
    /// # Safety
    ///
    /// Taking static immutable reference here is safe because BOOT_INFO is only set once and no
    /// one will take a mutable reference to it.
    pub fn take() -> &'static Self {
        unsafe { PCI.as_ref().expect("PCI is not initialized yet") }
    }
    /// # Safety
    ///
    /// This function panics when it is called twice, to ensure that Some(boot_info) has a "static"
    /// lifetime
    pub unsafe fn set(pci: Self) {
        assert!(PCI.is_none());
        PCI = Some(pci);
    }
}
unsafe impl Sync for Pci {
    // This Sync impl is fake
    // but read access to it will be safe
}
static mut PCI: Option<Pci> = None;

#[cfg(test)]
mod tests {
    use super::*;
    #[test_case]
    fn construct_bus_device_function() {
        let bus = 11;
        let device = 7;
        let function = 5;
        let bdf = BusDeviceFunction::new(bus, device, function)
            .expect("Failed to construct BusDeviceFunction");
        assert!(bdf.bus() == bus);
        assert!(bdf.device() == device);
        assert!(bdf.function() == function);
    }
    #[test_case]
    fn bus_device_function_iterator() {
        let it = BusDeviceFunction::iter();
        let mut count = 0;
        for (i, bdf) in it.enumerate() {
            assert!(bdf.bus() == i >> 8);
            assert!(bdf.device() == (i >> 3) & 0b11111);
            assert!(bdf.function() == i & 0b111);
            count += 1;
        }
        assert_eq!(count, 0x10000);
    }
}
