extern crate alloc;

use crate::arch::x86_64::busy_loop_hint;
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
use core::mem::ManuallyDrop;
use core::ptr::read_volatile;
use core::ptr::write_volatile;

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

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
#[repr(C)]
struct OperationalRegisters {
    command: u32,
    status: u32,
    page_size: u32,
    rsvdz1: [u32; 2],
    notification_ctrl: u32,
    cmd_ring_ctrl: u64,
    rsvdz2: [u64; 2],
    device_ctx_base_addr_array_ptr: u64,
    config: u64,
}
impl OperationalRegisters {
    fn clear_command_bits(&mut self, bits: u32) {
        unsafe {
            write_volatile(&mut self.command, self.command() & !bits);
        }
    }
    fn set_command_bits(&mut self, bits: u32) {
        unsafe {
            write_volatile(&mut self.command, self.command() | bits);
        }
    }
    fn command(&mut self) -> u32 {
        unsafe { read_volatile(&self.command) }
    }
    fn status(&mut self) -> u32 {
        unsafe { read_volatile(&self.status) }
    }
}
const _: () = assert!(core::mem::size_of::<OperationalRegisters>() == 0x40);

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
#[repr(C)]
struct InterrupterRegisterSet {
    management: u32,
    moderation: u32,
    erst_size: u32,
    rsvdp: u32,
    erst_base: u64,
    erdp: u64,
}
const _: () = assert!(core::mem::size_of::<InterrupterRegisterSet>() == 0x20);
#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
#[repr(C)]
struct RuntimeRegisters {
    microframe_index: u32,
    rsvdz: [u32; 7],
    irs: [InterrupterRegisterSet; 1024],
}
const _: () = assert!(core::mem::size_of::<RuntimeRegisters>() == 0x8020);
//  static_assert(offsetof(RuntimeRegisters, irs) == 0x20);

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
    max_num_scratch_pad_bufs: usize,
    cap_regs: ManuallyDrop<Box<CapabilityRegisters>>,
    op_regs: ManuallyDrop<Box<OperationalRegisters>>,
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

        let cap_regs =
            unsafe { ManuallyDrop::new(Box::from_raw(bar0.addr() as *mut CapabilityRegisters)) };
        let op_regs = unsafe {
            ManuallyDrop::new(Box::from_raw(
                bar0.addr().add(cap_regs.length as usize) as *mut OperationalRegisters
            ))
        };
        let hcs_params1 = unsafe { read_volatile(&cap_regs.params[0] as *const u32) };
        let max_slots = extract_bits(hcs_params1, 24, 8) as usize;
        let max_ports = extract_bits(hcs_params1, 0, 8) as usize;

        let hcs_params2 = unsafe { read_volatile(&cap_regs.params[0] as *const u32) };
        let max_num_scratch_pad_bufs =
            (extract_bits(hcs_params2, 21, 5) << 5 | extract_bits(hcs_params2, 27, 5)) as usize;

        let mut xhc = XhciDriverInstance {
            bdf,
            max_slots,
            max_ports,
            max_num_scratch_pad_bufs,
            cap_regs,
            op_regs,
        };
        println!("{:?}", xhc);
        xhc.reset();

        unimplemented!()
    }
    fn reset(&mut self) {
        const CMD_RUN_STOP: u32 = 0b0001;
        const CMD_HC_RESET: u32 = 0b0010;
        const STATUS_HC_HALTED: u32 = 0b0001;
        println!("[xHC] Resetting the controller...");
        self.op_regs.clear_command_bits(CMD_RUN_STOP);
        while self.op_regs.status() & STATUS_HC_HALTED == 0 {
            busy_loop_hint();
        }
        self.op_regs.set_command_bits(CMD_HC_RESET);
        while self.op_regs.command() & CMD_HC_RESET != 0 {
            busy_loop_hint();
        }
        println!("[xHC] Reset done!");
    }
}
impl PciDeviceDriverInstance for XhciDriverInstance {
    fn name(&self) -> &str {
        "XhciDriverInstance"
    }
}
