extern crate alloc;

use crate::arch::x86_64::paging::with_current_page_table;
use crate::arch::x86_64::paging::PageAttr;
use crate::error::Result;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::println;
use crate::util::extract_bits;
use alloc::boxed::Box;
use core::ptr::read_volatile;

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
    fn attach(&self, bdf: BusDeviceFunction) -> Result<Box<dyn PciDeviceDriverInstance>> {
        Ok(Box::new(XhciDriverInstance::new(bdf)?) as Box<dyn PciDeviceDriverInstance>)
    }
    fn name(&self) -> &str {
        "XhciDriver"
    }
}
#[allow(unused)]
#[derive(Debug)]
pub struct XhciDriverInstance {
    #[allow(dead_code)]
    bdf: BusDeviceFunction,
    max_slots: usize,
    max_ports: usize,
}
impl XhciDriverInstance {
    // https://wiki.osdev.org/RTL8139
    fn new(bdf: BusDeviceFunction) -> Result<Self> {
        let pci = Pci::take();
        pci.disable_interrupt(bdf)?;
        pci.enable_bus_master(bdf)?;
        let bar0 = pci.try_bar0_mem64(bdf)?;

        let vstart = bar0.addr() as u64;
        let vend = bar0.addr() as u64 + bar0.size();
        unsafe {
            with_current_page_table(|pt| {
                pt.create_mapping(vstart, vend, vstart, PageAttr::ReadWriteIo)
                    .expect("Failed to create mapping")
            })
        }
        println!("page_table updated!");

        let cap_regs = unsafe { &*(bar0.addr() as *const CapabilityRegisters) };
        println!("cap_regs @ {:p}", cap_regs);
        println!("params[0] @ {:p}", &cap_regs.params[0]);

        let hcs_params1 = unsafe { read_volatile(&cap_regs.params[0] as *const u32) };
        println!("hcs_params1 = {:X}", hcs_params1);
        let max_slots = extract_bits(hcs_params1, 24, 8) as usize;
        let max_ports = extract_bits(hcs_params1, 0, 8) as usize;

        let di = XhciDriverInstance {
            bdf,
            max_slots,
            max_ports,
        };
        println!("{:?}", di);

        unimplemented!()
    }
}
impl PciDeviceDriverInstance for XhciDriverInstance {
    fn name(&self) -> &str {
        "XhciDriverInstance"
    }
}
