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
use crate::print;
use crate::println;
use crate::util::extract_bits;
use alloc::boxed::Box;
use core::mem::ManuallyDrop;
use core::mem::MaybeUninit;
use core::ptr::read_volatile;
use core::ptr::write_volatile;

#[repr(u32)]
#[non_exhaustive]
enum TrbType {
    Link = 6,
    NoOp = 8,
}

#[repr(u32)]
#[non_exhaustive]
enum TrbControl {
    CycleBit = 1,
    ToggleCycle = 2,
}

#[derive(Debug)]
#[repr(C, align(16))]
struct GenericTrbEntry {
    data: u64,
    option: u32,
    control: u32,
}
const _: () = assert!(core::mem::size_of::<GenericTrbEntry>() == 16);
impl GenericTrbEntry {
    fn set_control(&mut self, trb_type: TrbType, trb_control: TrbControl) {
        self.control = (trb_type as u32) << 10 | trb_control as u32;
    }
}

#[derive(Debug)]
#[repr(C, align(4096))]
struct TrbRing {
    trb: [GenericTrbEntry; 256],
}
const _: () = assert!(core::mem::size_of::<TrbRing>() == 4096);
impl TrbRing {
    fn new() -> Box<Self> {
        let mut ring: Box<Self> = Box::new(unsafe { MaybeUninit::zeroed().assume_init() });
        ring.init();
        ring
    }
    fn init(&mut self) {
        let trb_head_paddr = self.phys_addr() as u64;
        let link = &mut self.trb[self.trb.len() - 1];
        link.data = trb_head_paddr;
        link.option = 0;
        link.set_control(TrbType::Link, TrbControl::ToggleCycle);
    }
    fn phys_addr(&self) -> u64 {
        &self.trb[0] as *const GenericTrbEntry as u64
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
            ring: TrbRing::new(),
            erst: EventRingSegmentTableEntry::alloc(),
            cycle_state: 0,
            index: 0,
        }
    }
    fn erst_phys_addr(&self) -> usize {
        self.erst.as_ref() as *const EventRingSegmentTableEntry as usize
    }
    fn ring(&self) -> &TrbRing {
        &self.ring
    }
}
#[derive(Debug)]
struct CommandRing {
    ring: Box<TrbRing>,
    cycle_state: u32,
    index: usize,
}
impl CommandRing {
    fn new() -> Self {
        Self {
            ring: TrbRing::new(),
            cycle_state: 0,
            index: 0,
        }
    }
    fn ring(&self) -> &TrbRing {
        &self.ring
    }
}

#[repr(C, align(64))]
struct DeviceContextBaseAddressArray {
    context: [u64; 256],
}
const _: () = assert!(core::mem::size_of::<DeviceContextBaseAddressArray>() == 2048);
impl DeviceContextBaseAddressArray {
    fn new() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
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
    const CMD_RUN_STOP: u32 = 0b0001;
    const CMD_HC_RESET: u32 = 0b0010;
    const STATUS_HC_HALTED: u32 = 0b0001;
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
    fn set_num_device_slots(&mut self, num: usize) -> Result<()> {
        unsafe {
            let c = read_volatile(&self.config);
            let c = c & !0xFF;
            let c = c | u64::try_from(num)?;
            write_volatile(&mut self.config, c);
        }
        Ok(())
    }
    fn set_dcbaa_ptr(&mut self, dcbaa: &DeviceContextBaseAddressArray) -> Result<()> {
        unsafe {
            write_volatile(
                &mut self.device_ctx_base_addr_array_ptr,
                u64::try_from(
                    dcbaa as &DeviceContextBaseAddressArray as *const DeviceContextBaseAddressArray
                        as usize,
                )?,
            );
        }
        Ok(())
    }
    fn reset_xhc(&mut self) {
        print!("[xHC] Resetting the controller...");
        self.clear_command_bits(Self::CMD_RUN_STOP);
        while self.status() & Self::STATUS_HC_HALTED == 0 {
            print!(".");
            busy_loop_hint();
        }
        self.set_command_bits(Self::CMD_HC_RESET);
        while self.command() & Self::CMD_HC_RESET != 0 {
            print!(".");
            busy_loop_hint();
        }
        println!("Done!");
    }
    fn start_xhc(&mut self) {
        print!("[xHC] Starting the controller...");
        self.set_command_bits(Self::CMD_RUN_STOP);
        while self.status() & Self::STATUS_HC_HALTED != 0 {
            print!(".");
            busy_loop_hint();
        }
        println!("Done!");
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
pub struct XhciDriverInstance {
    #[allow(dead_code)]
    bdf: BusDeviceFunction,
    cap_regs: ManuallyDrop<Box<CapabilityRegisters>>,
    op_regs: ManuallyDrop<Box<OperationalRegisters>>,
    rt_regs: ManuallyDrop<Box<RuntimeRegisters>>,
    command_ring: CommandRing,
    primary_event_ring: EventRing,
    device_contexts: DeviceContextBaseAddressArray,
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
            command_ring: CommandRing::new(),
            primary_event_ring: EventRing::new(),
            device_contexts: DeviceContextBaseAddressArray::new(),
        };
        xhc.op_regs.reset_xhc();
        xhc.init_primary_interrupter()?;
        xhc.init_slots_and_contexts()?;
        xhc.init_command_ring()?;
        xhc.op_regs.start_xhc();
        Ok(xhc)
    }
    fn init_primary_interrupter(&mut self) -> Result<()> {
        let eq = &self.primary_event_ring;
        let dst = &mut self.rt_regs.irs[0];
        dst.erst_size = 1;
        dst.erdp = eq.ring().phys_addr() as u64;
        dst.erst_base = eq.ring().phys_addr() as u64;
        Ok(())
    }
    fn init_slots_and_contexts(&mut self) -> Result<()> {
        let num_slots = self.cap_regs.num_of_device_slots();
        println!("num_slots = {}", num_slots);
        self.op_regs.set_num_device_slots(num_slots);
        self.op_regs.set_dcbaa_ptr(&self.device_contexts);
        Ok(())
    }
    fn init_command_ring(&mut self) -> Result<()> {
        self.op_regs.cmd_ring_ctrl = self.command_ring.ring().phys_addr() | 1 /* Ring Cycle State */;
        Ok(())
    }
}
impl PciDeviceDriverInstance for XhciDriverInstance {
    fn name(&self) -> &str {
        "XhciDriverInstance"
    }
}
