extern crate alloc;

use crate::error::Result;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::println;
use alloc::boxed::Box;

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
#[repr(C)]
struct CapabilityRegisters {
    length: u8,
    reserved: u8,
    version: u16,
    params: [u32; 3],
    cap_params1: u32,
    dboff: u32,
    rtsoff: u32,
    cap_params2: u32,
}
const _: () = assert!(core::mem::size_of::<CapabilityRegisters>() == 0x20);

pub struct XhciDriver {}
impl XhciDriver {
    pub fn default() -> Self {
        XhciDriver {}
    }
}
impl PciDeviceDriver for XhciDriver {
    fn supports(&self, vp: VendorDeviceId) -> bool {
        const VDI_LIST: [VendorDeviceId; 2] = [
            VendorDeviceId {
                vendor: 0x1b36,
                device: 0x000d,
            },
            VendorDeviceId {
                vendor: 0x8086,
                device: 0x31a8,
            },
        ];
        VDI_LIST.contains(&vp)
    }
    fn attach(&self, bdf: BusDeviceFunction) -> Result<Box<dyn PciDeviceDriverInstance>> {
        Ok(Box::new(XhciDriverInstance::new(bdf)?) as Box<dyn PciDeviceDriverInstance>)
    }
    fn name(&self) -> &str {
        "XhciDriver"
    }
}
pub struct XhciDriverInstance {
    #[allow(dead_code)]
    bdf: BusDeviceFunction,
}
impl XhciDriverInstance {
    // https://wiki.osdev.org/RTL8139
    fn new(bdf: BusDeviceFunction) -> Result<Self> {
        let pci = Pci::take();
        pci.disable_interrupt(bdf)?;
        pci.enable_bus_master(bdf)?;
        let bar0 = pci.try_bar0_mem64(bdf)?;
        println!("{:p}", bar0.addr());
        let cap_regs = unsafe { &*(bar0.addr() as *const CapabilityRegisters) };
        println!("{:p}", cap_regs);
        println!("{:?}", cap_regs.params[0]);

        unimplemented!()
    }
}
impl PciDeviceDriverInstance for XhciDriverInstance {
    fn name(&self) -> &str {
        "XhciDriverInstance"
    }
}
