extern crate alloc;

use crate::allocator::ALLOCATOR;
use crate::arch::x86_64::busy_loop_hint;
use crate::arch::x86_64::paging::with_current_page_table;
use crate::arch::x86_64::paging::PageAttr;
use crate::error::Result;
use crate::error::WasabiError;
use crate::pci::BarMem64;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::print;
use crate::println;
use crate::util::extract_bits;
use alloc::alloc::Layout;
use alloc::boxed::Box;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::fmt::Display;
use alloc::format;
use alloc::string::String;
use core::mem::ManuallyDrop;
use core::mem::MaybeUninit;
use core::ptr::read_volatile;
use core::ptr::write_volatile;
use core::slice;

#[repr(u32)]
#[non_exhaustive]
enum TrbType {
    Link = 6,
    NoOpCommand = 23,
}

#[repr(u32)]
#[non_exhaustive]
enum TrbControl {
    None = 0,
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
    fn cycle_state(&self) -> u32 {
        self.control & (TrbControl::CycleBit as u32)
    }
    fn flip_cycle_state(&mut self) {
        self.control ^= TrbControl::CycleBit as u32
    }
    fn cmd_no_op() -> Self {
        let mut trb = Self {
            data: 0,
            option: 0,
            control: 0,
        };
        trb.set_control(TrbType::NoOpCommand, TrbControl::None);
        trb
    }
}
fn error_stringify<T: Debug>(x: T) -> String {
    format!("[xHC] Error: {x:?}")
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
    fn num_trbs(&self) -> usize {
        self.trb.len()
    }
}

struct EventRing {
    ring: Box<TrbRing>,
    erst: Box<EventRingSegmentTableEntry>,
    cycle_state_ours: u32,
    next_dequeue_index: usize,
}
impl EventRing {
    fn new() -> Result<Self> {
        let ring = TrbRing::new();
        let erst = EventRingSegmentTableEntry::new(&ring)?;
        Ok(Self {
            ring,
            erst,
            cycle_state_ours: 1,
            next_dequeue_index: 0,
        })
    }
    fn erst_phys_addr(&self) -> usize {
        self.erst.as_ref() as *const EventRingSegmentTableEntry as usize
    }
    fn ring(&self) -> &TrbRing {
        &self.ring
    }
    fn has_next_event(&self) -> bool {
        self.ring().trb[self.next_dequeue_index].cycle_state() == self.cycle_state_ours
    }
}
#[derive(Debug)]
struct CommandRing {
    ring: Box<TrbRing>,
    cycle_state_ours: u32,
    next_enqueue_index: usize,
}
impl CommandRing {
    fn new() -> Self {
        Self {
            ring: TrbRing::new(),
            cycle_state_ours: 0,
            next_enqueue_index: 0,
        }
    }
    fn ring(&self) -> &TrbRing {
        &self.ring
    }
    fn ring_mut(&mut self) -> &mut TrbRing {
        &mut self.ring
    }
    fn advance_enque_index(&mut self) -> Result<()> {
        let index_to_send = self.next_enqueue_index;
        let next_index = (self.next_enqueue_index + 1) % self.ring().trb.len();
        let (next_index, link_trb_index) = if next_index == self.ring().trb.len() - 2 {
            (0, Some(next_index))
        } else {
            (next_index, None)
        };
        // Ensure that we own next TRBs
        let cycle_state_ours = self.cycle_state_ours;
        let trb = &mut self.ring_mut().trb;
        if trb[next_index].cycle_state() != cycle_state_ours {
            return Err(WasabiError::Failed("Command Ring is Full"));
        }
        if let Some(link_trb_index) = link_trb_index {
            if trb[link_trb_index].cycle_state() != cycle_state_ours {
                return Err(WasabiError::Failed("Command Ring is Full"));
            }
        }
        trb[index_to_send].flip_cycle_state();
        if link_trb_index.is_some() {
            self.cycle_state_ours ^= 1;
        }
        Ok(())
    }
    fn push(&mut self, mut e: GenericTrbEntry) -> Result<()> {
        if e.cycle_state() != self.cycle_state_ours {
            e.flip_cycle_state();
        }
        let index = self.next_enqueue_index;
        self.ring_mut().trb[index] = e;
        self.advance_enque_index()?;
        Ok(())
    }
}

#[repr(C, align(64))]
struct RawDeviceContextBaseAddressArray {
    context: [u64; 256],
}
const _: () = assert!(core::mem::size_of::<RawDeviceContextBaseAddressArray>() == 2048);
impl RawDeviceContextBaseAddressArray {
    fn new() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

struct DeviceContextBaseAddressArray {
    inner: RawDeviceContextBaseAddressArray,
    _scratchpad_buffers: Box<[*mut u8]>,
}
impl DeviceContextBaseAddressArray {
    fn new(scratchpad_buffers: Box<[*mut u8]>) -> Self {
        let mut inner = RawDeviceContextBaseAddressArray::new();
        inner.context[0] = scratchpad_buffers.as_ptr() as u64;
        Self {
            inner,
            _scratchpad_buffers: scratchpad_buffers,
        }
    }
    fn inner_mut_ptr(&mut self) -> *mut RawDeviceContextBaseAddressArray {
        &mut self.inner as *mut RawDeviceContextBaseAddressArray
    }
}

struct EventRingSegmentTableEntry {
    ring_segment_base_address: u64,
    ring_segment_size: u16,
    _rsvdz: [u16; 3],
}
const _: () = assert!(core::mem::size_of::<EventRingSegmentTableEntry>() == 16);
impl EventRingSegmentTableEntry {
    fn new(ring: &TrbRing) -> Result<Box<Self>> {
        let mut erst: Box<Self> = Box::new(unsafe { MaybeUninit::zeroed().assume_init() });
        erst.ring_segment_base_address = ring.phys_addr();
        erst.ring_segment_size = ring.num_trbs().try_into()?;
        Ok(erst)
    }
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct CapabilityRegisters {
    length: u8,
    reserved: u8,
    version: u16,
    hcsparams1: u32,
    hcsparams2: u32,
    hcsparams3: u32,
    cparams1: u32,
    dboff: u32,
    rtsoff: u32,
    cparams2: u32,
}
const _: () = assert!(core::mem::size_of::<CapabilityRegisters>() == 0x20);
impl CapabilityRegisters {
    pub fn hcsparams1(&self) -> u32 {
        unsafe { read_volatile(&self.hcsparams1) }
    }
    pub fn hcsparams2(&self) -> u32 {
        unsafe { read_volatile(&self.hcsparams2) }
    }
    pub fn num_of_device_slots(&self) -> usize {
        extract_bits(self.hcsparams1(), 0, 8) as usize
    }
    pub fn num_of_interrupters(&self) -> usize {
        extract_bits(self.hcsparams1(), 8, 11) as usize
    }
    pub fn num_of_ports(&self) -> usize {
        extract_bits(self.hcsparams1(), 24, 8) as usize
    }
    pub fn num_scratch_pad_bufs(&self) -> usize {
        (extract_bits(self.hcsparams2(), 21, 5) << 5 | extract_bits(self.hcsparams2(), 27, 5))
            as usize
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
    device_ctx_base_addr_array_ptr: *mut RawDeviceContextBaseAddressArray,
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
    fn page_size(&self) -> Result<usize> {
        let page_size_bits = unsafe { read_volatile(&self.page_size) } & 0xFFFF;
        // bit[n] of page_size_bits is set => PAGE_SIZE will be 2^(n+12).
        if page_size_bits.count_ones() != 1 {
            return Err(WasabiError::Failed("PAGE_SIZE has multiple bits set"));
        }
        let page_size_shift = page_size_bits.trailing_zeros();
        Ok(1 << (page_size_shift + 12))
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
    fn set_dcbaa_ptr(&mut self, dcbaa: &mut DeviceContextBaseAddressArray) -> Result<()> {
        unsafe {
            write_volatile(
                &mut self.device_ctx_base_addr_array_ptr,
                dcbaa.inner_mut_ptr(),
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
    rsvdz: [u32; 8],
    irs: [InterrupterRegisterSet; 1024],
}
const _: () = assert!(core::mem::size_of::<RuntimeRegisters>() == 0x8020);

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

mod regs {
    use super::*;
    // [xhci] 5.4.8: PORTSC
    // OperationalBase + (0x400 + 0x10 * (n - 1))
    // where n = Port Number (1, 2, ..., MaxPorts)
    #[repr(C)]
    pub struct PortScWrapper {
        ptr: *mut u32,
    }
    #[derive(Debug)]
    pub enum PortScState {
        // Figure 4-25: USB2 Root Hub Port State Machine
        PoweredOff,
        Disconnected,
        Reset,
        Disabled,
        Enabled,
        Other {
            pp: bool,
            ccs: bool,
            ped: bool,
            pr: bool,
        },
    }
    impl PortScWrapper {
        const BIT_CURRENT_CONNECT_STATUS: u32 = 1 << 0;
        const BIT_PORT_ENABLED_DISABLED: u32 = 1 << 1;
        const BIT_PORT_RESET: u32 = 1 << 4;
        const BIT_PORT_POWER: u32 = 1 << 9;
        pub fn value(&self) -> u32 {
            unsafe { read_volatile(self.ptr) }
        }
        pub fn set_bits(&self, bits: u32) {
            let old = self.value();
            unsafe { write_volatile(self.ptr, old | bits) }
        }
        pub fn reset(&self) {
            self.set_bits(Self::BIT_PORT_POWER);
            while !self.pp() {
                busy_loop_hint();
            }
            self.set_bits(Self::BIT_PORT_RESET);
            while self.pr() {
                busy_loop_hint();
            }
        }
        pub fn ccs(&self) -> bool {
            // CCS - Current Connect Status - ROS
            self.value() & Self::BIT_CURRENT_CONNECT_STATUS != 0
        }
        pub fn ped(&self) -> bool {
            // PED - Port Enabled/Disabled - RW1CS
            self.value() & Self::BIT_PORT_ENABLED_DISABLED != 0
        }
        pub fn pr(&self) -> bool {
            // PR - Port Reset - RW1S
            self.value() & Self::BIT_PORT_RESET != 0
        }
        pub fn pp(&self) -> bool {
            // PP - Port Power - RWS
            self.value() & Self::BIT_PORT_POWER != 0
        }
        pub fn state(&self) -> PortScState {
            // 4.19.1.1 USB2 Root Hub Port
            match (self.pp(), self.ccs(), self.ped(), self.pr()) {
                (false, false, false, false) => PortScState::PoweredOff,
                (true, false, false, false) => PortScState::Disconnected,
                (true, true, false, false) => PortScState::Disabled,
                (true, true, false, true) => PortScState::Reset,
                (true, true, true, false) => PortScState::Enabled,
                tuple => {
                    let (pp, ccs, ped, pr) = tuple;
                    PortScState::Other { pp, ccs, ped, pr }
                }
            }
        }
    }
    impl Display for PortScWrapper {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "PORTSC: ")
        }
    }
    // Iterator over PortSc
    pub struct PortScIteratorItem {
        pub port: usize,
        pub portsc: PortScWrapper,
    }
    pub struct PortScIterator<'a> {
        list: &'a PortSc,
        next_index: usize,
    }
    impl<'a> Iterator for PortScIterator<'a> {
        type Item = PortScIteratorItem;
        fn next(&mut self) -> Option<Self::Item> {
            if self.next_index >= self.list.num_ports {
                None
            } else {
                let item = Self::Item {
                    port: self.next_index + 1,
                    portsc: PortScWrapper {
                        ptr: unsafe { self.list.base.add(self.next_index * 4) },
                    },
                };
                self.next_index += 1;
                Some(item)
            }
        }
    }
    // Interface to access PORTSC registers
    pub struct PortSc {
        base: *mut u32,
        num_ports: usize,
    }
    impl PortSc {
        pub fn new(bar: &BarMem64, cap_regs: &CapabilityRegisters) -> Self {
            let base = unsafe { bar.addr().add(cap_regs.length as usize).add(0x400) } as *mut u32;
            let num_ports = cap_regs.num_of_ports();
            println!("PORTSC @ {:p}, max_port_num = {}", base, num_ports);
            Self { base, num_ports }
        }
        pub fn iter(&self) -> PortScIterator {
            PortScIterator {
                list: self,
                next_index: 0,
            }
        }
    }
}

#[allow(unused)]
pub struct XhciDriverInstance {
    #[allow(dead_code)]
    bdf: BusDeviceFunction,
    cap_regs: ManuallyDrop<Box<CapabilityRegisters>>,
    op_regs: ManuallyDrop<Box<OperationalRegisters>>,
    rt_regs: ManuallyDrop<Box<RuntimeRegisters>>,
    portsc: regs::PortSc,
    doorbell_regs: ManuallyDrop<Box<[u32; 256]>>,
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
        let mut op_regs = unsafe {
            ManuallyDrop::new(Box::from_raw(
                bar0.addr().add(cap_regs.length as usize) as *mut OperationalRegisters
            ))
        };
        op_regs.reset_xhc();
        let rt_regs = unsafe {
            ManuallyDrop::new(Box::from_raw(
                bar0.addr().add(cap_regs.rtsoff as usize) as *mut RuntimeRegisters
            ))
        };
        let doorbell_regs = unsafe {
            ManuallyDrop::new(Box::from_raw(
                bar0.addr().add(cap_regs.dboff as usize) as *mut [u32; 256]
            ))
        };
        let portsc = regs::PortSc::new(&bar0, &cap_regs);
        let scratchpad_buffers =
            Self::alloc_scratch_pad_buffers(op_regs.page_size()?, cap_regs.num_scratch_pad_bufs())?;
        let device_contexts = DeviceContextBaseAddressArray::new(scratchpad_buffers);
        let mut xhc = XhciDriverInstance {
            bdf,
            cap_regs,
            op_regs,
            rt_regs,
            portsc,
            doorbell_regs,
            command_ring: CommandRing::new(),
            primary_event_ring: EventRing::new()?,
            device_contexts,
        };
        xhc.init_primary_event_ring()?;
        xhc.init_slots_and_contexts()?;
        xhc.init_command_ring()?;
        xhc.op_regs.start_xhc();

        xhc.ensure_ring_is_working()?;
        xhc.poll_ports()?;

        Ok(xhc)
    }
    fn init_primary_event_ring(&mut self) -> Result<()> {
        let eq = &self.primary_event_ring;
        let irs = &mut self.rt_regs.irs[0];
        irs.erst_size = 1;
        irs.erdp = eq.ring().phys_addr() as u64;
        irs.erst_base = eq.erst_phys_addr() as u64;
        irs.management = 0;
        Ok(())
    }
    fn init_slots_and_contexts(&mut self) -> Result<()> {
        let num_slots = self.cap_regs.num_of_device_slots();
        println!("num_slots = {}", num_slots);
        self.op_regs.set_num_device_slots(num_slots)?;
        self.op_regs.set_dcbaa_ptr(&mut self.device_contexts)?;
        Ok(())
    }
    fn init_command_ring(&mut self) -> Result<()> {
        self.op_regs.cmd_ring_ctrl = self.command_ring.ring().phys_addr() | 1 /* Ring Cycle State */;
        Ok(())
    }
    fn alloc_scratch_pad_buffers(
        page_size: usize,
        num_scratch_pad_bufs: usize,
    ) -> Result<Box<[*mut u8]>> {
        // 4.20 Scratchpad Buffers
        // This should be done before xHC starts.
        // device_contexts.context[0] points Scratchpad Buffer Arrary.
        // The array contains pointers to a memory region which is sized PAGESIZE and aligned on
        // PAGESIZE. (PAGESIZE can be retrieved from op_regs.PAGESIZE)
        println!("PAGE_SIZE = {}", page_size);
        println!("num_scratch_pad_bufs = {}", num_scratch_pad_bufs);
        let scratchpad_buffers = ALLOCATOR.alloc_with_options(
            Layout::from_size_align(
                core::mem::size_of::<usize>() * num_scratch_pad_bufs,
                page_size,
            )
            .map_err(error_stringify)?,
        );
        let scratchpad_buffers = unsafe {
            slice::from_raw_parts(scratchpad_buffers as *mut *mut u8, num_scratch_pad_bufs)
        };
        let mut scratchpad_buffers = Box::<[*mut u8]>::from(scratchpad_buffers);
        for sb in scratchpad_buffers.iter_mut() {
            *sb = ALLOCATOR.alloc_with_options(
                Layout::from_size_align(page_size, page_size).map_err(error_stringify)?,
            );
        }
        Ok(scratchpad_buffers)
    }
    fn notify_xhc(&mut self) {
        unsafe {
            write_volatile(&mut self.doorbell_regs[0], 0);
        }
    }
    fn ensure_ring_is_working(&mut self) -> Result<()> {
        self.command_ring.push(GenericTrbEntry::cmd_no_op())?;
        self.notify_xhc();
        print!("No Op Command is sent. Waiting for an event...");
        while !self.primary_event_ring.has_next_event() {
            busy_loop_hint();
        }
        println!("Done!");
        Ok(())
    }
    fn poll_ports(&mut self) -> Result<()> {
        for regs::PortScIteratorItem { port, portsc } in self.portsc.iter() {
            let state = portsc.state();
            println!(
                "portsc[port = {}] = {:#10X} {:?}",
                port,
                portsc.value(),
                portsc.state()
            );
            if let regs::PortScState::Disconnected = state {
                // Reset port to Enable the port (via Reset state)
                println!("Resetting port {}...", portsc.value());
                portsc.reset();
                println!("Done!")
            }
        }
        Ok(())
    }
}
impl PciDeviceDriverInstance for XhciDriverInstance {
    fn name(&self) -> &str {
        "XhciDriverInstance"
    }
}
