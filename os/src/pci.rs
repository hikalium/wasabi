use crate::acpi::Mcfg;
use crate::println;
use core::ops::Range;

pub struct Pci {
    ecm_range: Range<usize>,
}
pub struct VendorDeviceId {
    pub vendor: u16,
    pub device: u16,
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
    fn ecm_base(&self, bus: usize, device: usize, function: usize) -> *mut u16 {
        assert!((0..256).contains(&bus));
        assert!((0..32).contains(&device));
        assert!((0..8).contains(&function));
        (self.ecm_range.start + (bus << 20) + (device << 15) + (function << 12)) as *mut u16
    }
    pub fn read_register_u16(
        &self,
        bus: usize,
        device: usize,
        function: usize,
        byte_offset: usize,
    ) -> u16 {
        assert!((0..256).contains(&byte_offset));
        assert!(byte_offset & 1 == 0);
        let ecm_base = self.ecm_base(bus, device, function);
        unsafe { *ecm_base.add(byte_offset >> 1) }
    }
    pub fn read_vendor_id_and_device_id(
        &self,
        bus: usize,
        device: usize,
        function: usize,
    ) -> Option<VendorDeviceId> {
        let vendor = self.read_register_u16(bus, device, function, 0);
        let device = self.read_register_u16(bus, device, function, 2);
        if vendor == 0xFFFF || device == 0xFFFF {
            // Not connected
            None
        } else {
            Some(VendorDeviceId { vendor, device })
        }
    }
    pub fn list_devices(&self) {
        for bus in 0..256 {
            for device in 0..32 {
                for function in 0..8 {
                    if let Some(VendorDeviceId { vendor, device }) =
                        self.read_vendor_id_and_device_id(bus, device, function)
                    {
                        println!("bus:{:#02X}, device:{:#02X}, function: {:#02X}, vendor_id: {:#04X}, device_id: {:#04X}", bus, device, function, vendor, device);
                    }
                }
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
