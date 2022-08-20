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
use core::mem::MaybeUninit;
use core::ptr::read_volatile;
use core::ptr::write_volatile;

#[derive(Debug)]
#[repr(C, align(16))]
struct GenericTrbEntry {
    data: u64,
    option: u32,
    control: u32,
}
const _: () = assert!(core::mem::size_of::<GenericTrbEntry>() == 16);

#[derive(Debug)]
#[repr(C, align(4096))]
struct TrbRing {
    trb: [GenericTrbEntry; 256],
}
const _: () = assert!(core::mem::size_of::<TrbRing>() == 4096);
impl TrbRing {
    fn alloc() -> Box<Self> {
        Box::new(unsafe { MaybeUninit::zeroed().assume_init() })
    }
}

#[derive(Debug)]
struct EventRingSegmentTableEntry {
    ring_segment_base_address: u64,
    ring_segment_size: u16,
    rsvdz: [u16; 3],
}
const _: () = assert!(core::mem::size_of::<EventRingSegmentTableEntry>() == 16);
impl EventRingSegmentTableEntry {
    fn alloc() -> Box<Self> {
        Box::new(unsafe { MaybeUninit::zeroed().assume_init() })
    }
}

#[derive(Debug)]
struct EventRing {
    ring: Box<TrbRing>,
    erst: Box<EventRingSegmentTableEntry>,
    cycle_state: u32,
    index: usize,
}
impl EventRing {
    fn new() -> Self {
        Self {
            ring: TrbRing::alloc(),
            erst: EventRingSegmentTableEntry::alloc(),
            cycle_state: 0,
            index: 0,
        }
    }
    fn ring_phys_addr(&self) -> usize {
        self.ring.as_ref() as *const TrbRing as usize
    }
    fn erst_phys_addr(&self) -> usize {
        self.erst.as_ref() as *const EventRingSegmentTableEntry as usize
    }
}

#[repr(u32)]
#[non_exhaustive]
enum TrbType {
    NoOp = 8,
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
#[repr(C)]
struct CapabilityRegisters {
    length: u8,
    reserved: u8,
    version: u16,
    sparams1: u32,
    sparams2: u32,
    sparams3: u32,
    cparams1: u32,
    dboff: u32,
    rtsoff: u32,
    cparams2: u32,
}
const _: () = assert!(core::mem::size_of::<CapabilityRegisters>() == 0x20);
impl CapabilityRegisters {
    fn num_of_device_slots(&self) -> usize {
        extract_bits(self.sparams1, 0, 8) as usize
    }
    fn num_of_interrupters(&self) -> usize {
        extract_bits(self.sparams1, 8, 11) as usize
    }
    fn num_of_ports(&self) -> usize {
        extract_bits(self.sparams1, 24, 8) as usize
    }
    fn num_scratch_pad_bufs(&self) -> usize {
        (extract_bits(self.sparams2, 21, 5) << 5 | extract_bits(self.sparams2, 27, 5)) as usize
    }
}

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
const _: () = assert!(core::mem::size_of::<OperationalRegisters>() == 0x40);
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
    cap_regs: ManuallyDrop<Box<CapabilityRegisters>>,
    op_regs: ManuallyDrop<Box<OperationalRegisters>>,
    rt_regs: ManuallyDrop<Box<RuntimeRegisters>>,
    primary_event_ring: EventRing,
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

        let cap_regs =
            unsafe { ManuallyDrop::new(Box::from_raw(bar0.addr() as *mut CapabilityRegisters)) };
        let op_regs = unsafe {
            ManuallyDrop::new(Box::from_raw(
                bar0.addr().add(cap_regs.length as usize) as *mut OperationalRegisters
            ))
        };
        let rt_regs = unsafe {
            ManuallyDrop::new(Box::from_raw(
                bar0.addr().add(cap_regs.rtsoff as usize) as *mut RuntimeRegisters
            ))
        };
        let mut xhc = XhciDriverInstance {
            bdf,
            cap_regs,
            op_regs,
            rt_regs,
            primary_event_ring: EventRing::new(),
        };
        xhc.reset();
        xhc.init_primary_interrupter()?;
        xhc.init_slots_and_contexts()?;
        Ok(xhc)
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
    fn init_primary_interrupter(&mut self) -> Result<()> {
        let eq = &self.primary_event_ring;
        let dst = &mut self.rt_regs.irs[0];
        dst.erst_size = 1;
        dst.erdp = eq.ring_phys_addr().try_into()?;
        dst.erst_base = eq.erst_phys_addr().try_into()?;
        Ok(())
    }
    fn init_slots_and_contexts(&mut self) -> Result<()> {
        let num_slots = self.cap_regs.num_of_device_slots();
        println!("num_slots = {}", num_slots);
        Ok(())
    }
}
impl PciDeviceDriverInstance for XhciDriverInstance {
    fn name(&self) -> &str {
        "XhciDriverInstance"
    }
}
