use crate::acpi::Mcfg;
use crate::error::Result;
use crate::error::WasabiError;
use crate::println;
use core::fmt;
use core::ops::Range;

pub struct VendorDeviceId {
    pub vendor: u16,
    pub device: u16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
}
impl fmt::Display for BusDeviceFunction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(bus: {:#04X}, device: {:#04X}, function: {:#03X})",
            self.bus(),
            self.device(),
            self.function()
        )
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

pub struct Pci {
    ecm_range: Range<usize>,
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
        Pci {
            ecm_range: pci_config_space_base..pci_config_space_end,
        }
    }
    fn ecm_base(&self, id: BusDeviceFunction) -> *mut u16 {
        (self.ecm_range.start + ((id.id as usize) << 12)) as *mut u16
    }
    pub fn read_register_u16(&self, id: BusDeviceFunction, byte_offset: usize) -> u16 {
        assert!((0..256).contains(&byte_offset));
        assert!(byte_offset & 1 == 0);
        let ecm_base = self.ecm_base(id);
        unsafe { *ecm_base.add(byte_offset >> 1) }
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
    pub fn list_devices(&self) {
        for id in BusDeviceFunction::iter() {
            if let Some(VendorDeviceId { vendor, device }) = self.read_vendor_id_and_device_id(id) {
                println!(
                    "{}: vendor_id: {:#04X}, device_id: {:#04X}",
                    id, vendor, device
                );
            }
        }
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
