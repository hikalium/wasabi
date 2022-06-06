use crate::acpi::Mcfg;
use crate::println;
use core::ops::Range;

pub struct Pci {
    ecm_range: Range<u64>,
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
        let pci_config_space_base = mcfg.entry(0).expect("Out of range").base_address();
        let pci_config_space_end = pci_config_space_base + (1 << 24);
        println!(
            "PCI config space is mapped at: [{:#018X},{:#018X})",
            pci_config_space_base, pci_config_space_end
        );
        Pci {
            ecm_range: pci_config_space_base..pci_config_space_end,
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
