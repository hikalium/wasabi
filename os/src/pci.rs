extern crate alloc;

use crate::acpi::Mcfg;
use crate::error::Result;
use crate::error::WasabiError;
use crate::print::hexdump;
use crate::println;
use crate::rtl8139::Rtl8139Driver;
use crate::xhci::XhciDriver;
use alloc::boxed::Box;
use alloc::collections::btree_map::BTreeMap;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;
use core::marker::PhantomData;
use core::mem::size_of;
use core::ops::Range;
use core::ptr::read_volatile;
use core::ptr::write_volatile;

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

pub struct CapabilityHeader {
    pub id: u8,
    pub next: u8,
}

pub struct CapabilityIterator<'a> {
    pci: &'a Pci,
    bdf: BusDeviceFunction,
    ptr: u8,
}
impl<'a> CapabilityIterator<'a> {
    pub fn new(pci: &'a Pci, bdf: BusDeviceFunction, ptr: u8) -> Self {
        Self { pci, bdf, ptr }
    }
}
impl<'a> Iterator for CapabilityIterator<'a> {
    type Item = &'a CapabilityHeader;
    fn next(&mut self) -> Option<Self::Item> {
        if self.ptr == 0 {
            None
        } else {
            let item = unsafe { &*self.pci.ecm_base::<CapabilityHeader>(self.bdf) };
            self.ptr = item.next;
            Some(item)
        }
    }
}

pub trait PciDeviceDriver {
    fn supports(&self, vp: VendorDeviceId) -> bool;
    fn attach(&self, bdf: BusDeviceFunction) -> Result<Box<dyn PciDeviceDriverInstance>>;
    fn name(&self) -> &str;
}
impl fmt::Debug for dyn PciDeviceDriver {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PciDeviceDriver{{ name: {} }}", self.name())
    }
}

pub trait PciDeviceDriverInstance {
    fn name(&self) -> &str;
}
impl fmt::Debug for dyn PciDeviceDriverInstance {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PciDeviceDriverInstance{{ name: {} }}", self.name())
    }
}

struct ConfigRegisters<T> {
    access_type: PhantomData<T>,
}
impl<T> ConfigRegisters<T> {
    fn read(ecm_base: *mut T, byte_offset: usize) -> Result<T> {
        if !(0..256).contains(&byte_offset) || byte_offset % size_of::<T>() != 0 {
            Err(WasabiError::PciEcmOutOfRange)
        } else {
            unsafe { Ok(read_volatile(ecm_base.add(byte_offset / size_of::<T>()))) }
        }
    }
    fn write(ecm_base: *mut T, byte_offset: usize, data: T) -> Result<()> {
        if !(0..256).contains(&byte_offset) || byte_offset % size_of::<T>() != 0 {
            Err(WasabiError::PciEcmOutOfRange)
        } else {
            unsafe { write_volatile(ecm_base.add(byte_offset / size_of::<T>()), data) }
            Ok(())
        }
    }
}

pub struct BarMem64 {
    addr: *mut u8,
    size: u64,
}
impl BarMem64 {
    pub fn addr(&self) -> *mut u8 {
        self.addr
    }
    pub fn size(&self) -> u64 {
        self.size
    }
}

pub struct Pci {
    ecm_range: Range<usize>,
    drivers: Vec<Rc<Box<dyn PciDeviceDriver>>>,
    devices: RefCell<BTreeMap<BusDeviceFunction, Rc<Box<dyn PciDeviceDriverInstance>>>>,
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

        let drivers = vec![
            Rc::new(Box::new(Rtl8139Driver::default()) as Box<dyn PciDeviceDriver>),
            Rc::new(Box::new(XhciDriver::default()) as Box<dyn PciDeviceDriver>),
        ];

        Pci {
            ecm_range: pci_config_space_base..pci_config_space_end,
            drivers,
            devices: RefCell::new(BTreeMap::new()),
        }
    }
    pub fn try_bar0_io(&self, bdf: BusDeviceFunction) -> Result<u16> {
        let bar0 = self.read_register_u32(bdf, 0x10)?;
        if bar0 & 0b11 == 0b01
        /* I/O space */
        {
            Ok((bar0 ^ 1) as u16)
        } else {
            Err(WasabiError::PciBarInvalid)
        }
    }
    pub fn try_bar0_mem64(&self, bdf: BusDeviceFunction) -> Result<BarMem64> {
        hexdump(unsafe { core::slice::from_raw_parts(self.ecm_base(bdf), 0x100) });
        let bar0 = self.read_register_u64(bdf, 0x10)?;
        println!("bar0: {:018X}", bar0);
        if bar0 & 0b0111 == 0b0100
        /* Memory, 64bit, Non-prefetchable */
        {
            let addr = (bar0 & !0b1111) as *mut u8;
            // Write all-1s to get the size of the region
            self.write_register_u64(bdf, 0x10, !0u64)?;
            let size = 1 + !(self.read_register_u64(bdf, 0x10)? & !0b1111);
            // Restore the original value
            self.write_register_u64(bdf, 0x10, bar0)?;
            Ok(BarMem64 { addr, size })
        } else {
            Err(WasabiError::PciBarInvalid)
        }
    }
    pub fn ecm_base<T>(&self, id: BusDeviceFunction) -> *mut T {
        (self.ecm_range.start + ((id.id as usize) << 12)) as *mut T
    }
    pub fn read_register_u8(&self, bdf: BusDeviceFunction, byte_offset: usize) -> Result<u8> {
        ConfigRegisters::read(self.ecm_base(bdf), byte_offset)
    }
    pub fn read_register_u16(&self, bdf: BusDeviceFunction, byte_offset: usize) -> Result<u16> {
        ConfigRegisters::read(self.ecm_base(bdf), byte_offset)
    }
    pub fn read_register_u32(&self, bdf: BusDeviceFunction, byte_offset: usize) -> Result<u32> {
        ConfigRegisters::read(self.ecm_base(bdf), byte_offset)
    }
    pub fn read_register_u64(&self, bdf: BusDeviceFunction, byte_offset: usize) -> Result<u64> {
        let lo = self.read_register_u32(bdf, byte_offset)?;
        let hi = self.read_register_u32(bdf, byte_offset + 4)?;
        Ok(((hi as u64) << 32) | (lo as u64))
    }
    pub fn write_register_u32(
        &self,
        bdf: BusDeviceFunction,
        byte_offset: usize,
        data: u32,
    ) -> Result<()> {
        ConfigRegisters::write(self.ecm_base(bdf), byte_offset, data)
    }
    pub fn write_register_u64(
        &self,
        bdf: BusDeviceFunction,
        byte_offset: usize,
        data: u64,
    ) -> Result<()> {
        let lo: u32 = data as u32;
        let hi: u32 = (data >> 32) as u32;
        self.write_register_u32(bdf, byte_offset, lo)?;
        self.write_register_u32(bdf, byte_offset + 4, hi)?;
        Ok(())
    }
    pub fn set_command_and_status_flags(&self, bdf: BusDeviceFunction, flags: u32) -> Result<()> {
        let cmd_and_status = self.read_register_u32(bdf, 0x04 /* Command and status */)?;
        self.write_register_u32(
            bdf,
            0x04, /* Command and status */
            flags | cmd_and_status,
        )
    }
    pub fn enable_bus_master(&self, bdf: BusDeviceFunction) -> Result<()> {
        self.set_command_and_status_flags(bdf, 1 << 2 /* Bus Master Enable */)
    }
    pub fn disable_interrupt(&self, bdf: BusDeviceFunction) -> Result<()> {
        self.set_command_and_status_flags(bdf, 1 << 10 /* Interrupt Disable */)
    }
    pub fn capabilities(&self, id: BusDeviceFunction) -> Option<CapabilityIterator> {
        let status = self.read_register_u16(id, 0x06).ok()?;

        if status & (1 << 4) != 0 {
            Some(CapabilityIterator::new(
                self,
                id,
                self.read_register_u8(id, 0x34).ok()?,
            ))
        } else {
            None
        }
    }
    pub fn read_vendor_id_and_device_id(&self, id: BusDeviceFunction) -> Option<VendorDeviceId> {
        let vendor = self.read_register_u16(id, 0).ok()?;
        let device = self.read_register_u16(id, 2).ok()?;
        if vendor == 0xFFFF || device == 0xFFFF {
            // Not connected
            None
        } else {
            Some(VendorDeviceId { vendor, device })
        }
    }
    pub fn probe_devices(&self) -> Result<()> {
        println!("Probing PCI devices...");
        for bdf in BusDeviceFunction::iter() {
            if let Some(vd) = self.read_vendor_id_and_device_id(bdf) {
                println!("{:?}: {:?}", bdf, vd);
                if self.devices.borrow_mut().contains_key(&bdf) {
                    continue;
                }
                for d in &self.drivers {
                    if d.supports(vd) {
                        match d.attach(bdf) {
                            Ok(di) => {
                                println!("{:?} driver is loaded for {:?}", di, bdf);
                                self.devices.borrow_mut().insert(bdf, Rc::new(di));
                            }
                            Err(e) => {
                                println!("Failed to attach {:?} for {:?}: {:?}", d, bdf, e);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
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
