use crate::bits::extract_bits;
use crate::info;
use crate::mmio::Mmio;
use crate::pci::BarMem64;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::VendorDeviceId;
use crate::result::Result;
use crate::volatile::Volatile;
use core::marker::PhantomPinned;

pub struct PciXhciDriver {}
impl PciXhciDriver {
    pub fn supports(vp: VendorDeviceId) -> bool {
        const VDI_LIST: [VendorDeviceId; 3] = [
            VendorDeviceId {
                vendor: 0x1b36,
                device: 0x000d,
            },
            VendorDeviceId {
                vendor: 0x8086,
                device: 0x31a8,
            },
            VendorDeviceId {
                vendor: 0x8086,
                device: 0x02ed,
            },
        ];
        VDI_LIST.contains(&vp)
    }
    pub fn setup_xhc_registers(
        bar0: &BarMem64,
    ) -> Result<(
        Mmio<CapabilityRegisters>,
        Mmio<OperationalRegisters>,
        Mmio<RuntimeRegisters>,
    )> {
        let cap_regs = unsafe { Mmio::from_raw(bar0.addr() as *mut CapabilityRegisters) };
        let op_regs = unsafe {
            Mmio::from_raw(
                bar0.addr().add(cap_regs.as_ref().caplength()) as *mut OperationalRegisters
            )
        };
        let rt_regs = unsafe {
            Mmio::from_raw(bar0.addr().add(cap_regs.as_ref().rtsoff()) as *mut RuntimeRegisters)
        };
        Ok((cap_regs, op_regs, rt_regs))
    }
    pub fn attach(pci: &Pci, bdf: BusDeviceFunction) -> Result<()> {
        info!("Xhci found at: {bdf:?}");
        pci.disable_interrupt(bdf)?;
        pci.enable_bus_master(bdf)?;
        let bar0 = pci.try_bar0_mem64(bdf)?;
        bar0.disable_cache();
        info!("xhci: {bar0:?}");
        let (cap_regs, op_regs, rt_regs) = Self::setup_xhc_registers(&bar0)?;
        info!(
            "xhci: cap_regs.MaxSlots = {}",
            cap_regs.as_ref().num_of_device_slots()
        );
        info!("xhci: op_regs.USBSTS = {}", op_regs.as_ref().usbsts());
        info!("xhci: rt_regs.MFINDEX = {}", rt_regs.as_ref().mfindex());
        Err("wip")
    }
}

#[repr(C)]
pub struct CapabilityRegisters {
    caplength: Volatile<u8>,
    reserved: Volatile<u8>,
    version: Volatile<u16>,
    hcsparams1: Volatile<u32>,
    hcsparams2: Volatile<u32>,
    hcsparams3: Volatile<u32>,
    hccparams1: Volatile<u32>,
    dboff: Volatile<u32>,
    rtsoff: Volatile<u32>,
    hccparams2: Volatile<u32>,
}
const _: () = assert!(size_of::<CapabilityRegisters>() == 0x20);
impl CapabilityRegisters {
    pub fn caplength(&self) -> usize {
        self.caplength.read() as usize
    }
    pub fn rtsoff(&self) -> usize {
        self.rtsoff.read() as usize
    }
    pub fn num_of_device_slots(&self) -> usize {
        extract_bits(self.hcsparams1.read(), 0, 8) as usize
    }
}

// [xhci_1_2] p.31
// The Device Context Base Address Array contains 256 Entries
// and supports up to 255 USB devices or hubs
// [xhci_1_2] p.59
// the first entry (SlotID = '0') in the Device Context Base
// Address Array is utilized by the xHCI Scratchpad mechanism.
#[repr(C, align(64))]
pub struct RawDeviceContextBaseAddressArray {
    context: [u64; 256],
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<RawDeviceContextBaseAddressArray>() == 2048);
#[repr(C)]
pub struct OperationalRegisters {
    usbcmd: Volatile<u32>,
    usbsts: Volatile<u32>,
    pagesize: Volatile<u32>,
    rsvdz1: [u32; 2],
    dnctrl: Volatile<u32>,
    crcr: Volatile<u64>,
    rsvdz2: [u64; 2],
    dcbaap: Volatile<*mut RawDeviceContextBaseAddressArray>,
    config: Volatile<u64>,
}
const _: () = assert!(size_of::<OperationalRegisters>() == 0x40);
impl OperationalRegisters {
    fn usbsts(&self) -> u32 {
        self.usbsts.read()
    }
}

#[repr(C)]
struct InterrupterRegisterSet {
    management: u32,
    moderation: u32,
    erst_size: u32,
    rsvdp: u32,
    erst_base: u64,
    erdp: u64,
}
const _: () = assert!(size_of::<InterrupterRegisterSet>() == 0x20);

#[repr(C)]
pub struct RuntimeRegisters {
    mfindex: Volatile<u32>,
    rsvdz: [u32; 7],
    irs: [InterrupterRegisterSet; 1024],
}
const _: () = assert!(size_of::<RuntimeRegisters>() == 0x8020);
impl RuntimeRegisters {
    fn mfindex(&self) -> u32 {
        self.mfindex.read()
    }
}
