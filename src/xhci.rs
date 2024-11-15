extern crate alloc;

use crate::allocator::ALLOCATOR;
use crate::bits::extract_bits;
use crate::executor::spawn_global;
use crate::executor::yield_execution;
use crate::info;
use crate::mmio::IoBox;
use crate::mmio::Mmio;
use crate::mutex::Mutex;
use crate::pci::BarMem64;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::VendorDeviceId;
use crate::result::Result;
use crate::volatile::Volatile;
use crate::x86::busy_loop_hint;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::rc::Weak;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::cmp::max;
use core::marker::PhantomPinned;
use core::mem::MaybeUninit;
use core::ops::Range;
use core::pin::Pin;
use core::ptr::read_volatile;
use core::ptr::write_volatile;
use core::slice;

struct XhcRegisters {
    cap_regs: Mmio<CapabilityRegisters>,
    op_regs: Mmio<OperationalRegisters>,
    rt_regs: Mmio<RuntimeRegisters>,
    portsc: PortSc,
}

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
    fn setup_xhc_registers(bar0: &BarMem64) -> Result<XhcRegisters> {
        let cap_regs = unsafe { Mmio::from_raw(bar0.addr() as *mut CapabilityRegisters) };
        let op_regs = unsafe {
            Mmio::from_raw(
                bar0.addr().add(cap_regs.as_ref().caplength()) as *mut OperationalRegisters
            )
        };
        let rt_regs = unsafe {
            Mmio::from_raw(bar0.addr().add(cap_regs.as_ref().rtsoff()) as *mut RuntimeRegisters)
        };
        let portsc = PortSc::new(bar0, cap_regs.as_ref());
        Ok(XhcRegisters {
            cap_regs,
            op_regs,
            rt_regs,
            portsc,
        })
    }
    pub fn attach(pci: &Pci, bdf: BusDeviceFunction) -> Result<()> {
        info!("Xhci found at: {bdf:?}");
        pci.disable_interrupt(bdf)?;
        pci.enable_bus_master(bdf)?;
        let bar0 = pci.try_bar0_mem64(bdf)?;
        bar0.disable_cache();
        let regs = Self::setup_xhc_registers(&bar0)?;
        let xhc = Controller::new(regs)?;
        spawn_global(Self::run(xhc));
        Ok(())
    }
    async fn run(xhc: Controller) -> Result<()> {
        info!(
            "xhci: cap_regs.MaxSlots = {}",
            xhc.regs.cap_regs.as_ref().num_of_device_slots()
        );
        info!(
            "xhci: op_regs.USBSTS = {}",
            xhc.regs.op_regs.as_ref().usbsts()
        );
        info!(
            "xhci: rt_regs.MFINDEX = {}",
            xhc.regs.rt_regs.as_ref().mfindex()
        );
        info!("PORTSC values for port {:?}", xhc.regs.portsc.port_range());
        for port in xhc.regs.portsc.port_range() {
            if let Some(e) = xhc.regs.portsc.get(port) {
                info!("  {port:3}: {:#010X}", e.value())
            }
        }
        let xhc = Rc::new(xhc);
        {
            let xhc = xhc.clone();
            spawn_global(async move {
                loop {
                    xhc.primary_event_ring.lock().poll().await?;
                    yield_execution().await;
                }
            })
        }
        Ok(())
    }
}

#[repr(C)]
struct CapabilityRegisters {
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
    fn caplength(&self) -> usize {
        self.caplength.read() as usize
    }
    fn rtsoff(&self) -> usize {
        self.rtsoff.read() as usize
    }
    fn num_of_device_slots(&self) -> usize {
        extract_bits(self.hcsparams1.read(), 0, 8) as usize
    }
    fn num_scratchpad_bufs(&self) -> usize {
        (extract_bits(self.hcsparams2.read(), 21, 5) << 5
            | extract_bits(self.hcsparams2.read(), 27, 5)) as usize
    }
    fn num_of_ports(&self) -> usize {
        extract_bits(self.hcsparams1.read(), 24, 8) as usize
    }
}

// [xhci_1_2] p.31
// The Device Context Base Address Array contains 256 Entries
// and supports up to 255 USB devices or hubs
// [xhci_1_2] p.59
// the first entry (SlotID = '0') in the Device Context Base
// Address Array is utilized by the xHCI Scratchpad mechanism.
#[repr(C, align(64))]
struct RawDeviceContextBaseAddressArray {
    scratchpad_table_ptr: *const *const u8,
    context: [u64; 255],
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<RawDeviceContextBaseAddressArray>() == 2048);
impl RawDeviceContextBaseAddressArray {
    fn new() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

#[repr(C)]
struct OperationalRegisters {
    usbcmd: Volatile<u32>,
    usbsts: Volatile<u32>,
    pagesize: Volatile<u32>,
    rsvdz1: [u32; 2],
    dnctrl: Volatile<u32>,
    crcr: Volatile<u64>,
    rsvdz2: [u64; 2],
    dcbaap: Volatile<*const RawDeviceContextBaseAddressArray>,
    config: Volatile<u64>,
}
const _: () = assert!(size_of::<OperationalRegisters>() == 0x40);
impl OperationalRegisters {
    const STATUS_HC_HALTED: u32 = 0b0001;
    const CMD_RUN_STOP: u32 = 0b0001;
    const CMD_HC_RESET: u32 = 0b0010;
    fn usbsts(&self) -> u32 {
        self.usbsts.read()
    }
    fn page_size(&self) -> Result<usize> {
        let page_size_bits = self.pagesize.read() & 0xFFFF;
        // bit[n] of page_size_bits is set => PAGE_SIZE will be 2^(n+12).
        if page_size_bits.count_ones() != 1 {
            return Err("PAGE_SIZE has multiple bits set");
        }
        let page_size_shift = page_size_bits.trailing_zeros();
        Ok(1 << (page_size_shift + 12))
    }
    fn reset_xhc(&mut self) {
        self.clear_command_bits(Self::CMD_RUN_STOP);
        while self.usbsts.read() & Self::STATUS_HC_HALTED == 0 {
            busy_loop_hint();
        }
        self.set_command_bits(Self::CMD_HC_RESET);
        while self.usbcmd.read() & Self::CMD_HC_RESET != 0 {
            busy_loop_hint();
        }
    }
    fn start_xhc(&mut self) {
        self.set_command_bits(Self::CMD_RUN_STOP);
        while self.usbsts() & Self::STATUS_HC_HALTED != 0 {
            busy_loop_hint();
        }
    }
    fn set_cmd_ring_ctrl(&mut self, ring: &CommandRing) {
        self.crcr.write(
            ring.ring_phys_addr() | 1, /* Consumer Ring Cycle State */
        )
    }
    fn set_dcbaa_ptr(&mut self, dcbaa: &mut DeviceContextBaseAddressArray) -> Result<()> {
        self.dcbaap.write(dcbaa.inner_mut_ptr());
        Ok(())
    }
    fn set_num_device_slots(&mut self, num: usize) -> Result<()> {
        let c = self.config.read();
        let c = c & !0xFF;
        let c = c | num as u64;
        self.config.write(c);
        Ok(())
    }
    fn set_command_bits(&mut self, bits: u32) {
        self.usbcmd.write(self.usbcmd.read() | bits)
    }
    fn clear_command_bits(&mut self, bits: u32) {
        self.usbcmd.write(self.usbcmd.read() & !bits)
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
struct RuntimeRegisters {
    mfindex: Volatile<u32>,
    rsvdz: [u32; 7],
    irs: [InterrupterRegisterSet; 1024],
}
const _: () = assert!(size_of::<RuntimeRegisters>() == 0x8020);
impl RuntimeRegisters {
    fn mfindex(&self) -> u32 {
        self.mfindex.read()
    }
    fn init_irs(&mut self, index: usize, ring: &mut EventRing) -> Result<()> {
        let irs = self.irs.get_mut(index).ok_or("Index out of range")?;
        irs.erst_size = 1;
        irs.erdp = ring.ring_phys_addr();
        irs.erst_base = ring.erst_phys_addr();
        irs.management = 0;
        ring.set_erdp(&mut irs.erdp as *mut u64);
        Ok(())
    }
}

struct ScratchpadBuffers {
    table: Pin<Box<[*const u8]>>,
    _bufs: Vec<Pin<Box<[u8]>>>,
}
impl ScratchpadBuffers {
    fn alloc(cap_regs: &CapabilityRegisters, op_regs: &OperationalRegisters) -> Result<Self> {
        let page_size = op_regs.page_size()?;
        info!("xhci: page_size = {page_size}");
        let num_scratchpad_bufs = cap_regs.num_scratchpad_bufs();
        info!("xhci: original num_scratchpad_bufs = {num_scratchpad_bufs}");

        let num_scratchpad_bufs = max(cap_regs.num_scratchpad_bufs(), 1);
        let table = ALLOCATOR.alloc_with_options(
            Layout::from_size_align(size_of::<usize>() * num_scratchpad_bufs, page_size)
                .map_err(|_| "could not allocate scratchpad buffer table")?,
        );
        let table = unsafe { slice::from_raw_parts(table as *mut *const u8, num_scratchpad_bufs) };
        let mut table = Pin::new(Box::<[*const u8]>::from(table));
        let mut bufs = Vec::new();
        for sb in table.iter_mut() {
            let buf = ALLOCATOR.alloc_with_options(
                Layout::from_size_align(page_size, page_size)
                    .map_err(|_| "could not allocated a scratchpad buffer")?,
            );
            let buf = unsafe { slice::from_raw_parts(buf as *const u8, page_size) };
            let buf = Pin::new(Box::<[u8]>::from(buf));
            *sb = buf.as_ref().as_ptr();
            bufs.push(buf);
        }
        Ok(Self { table, _bufs: bufs })
    }
}

#[repr(C, align(32))]
#[derive(Default, Debug)]
struct EndpointContext {
    data: [u32; 2],
    tr_dequeue_ptr: Volatile<u64>,
    average_trb_length: u16,
    max_esit_payload_low: u16,
    _reserved: [u32; 3],
}
const _: () = assert!(size_of::<EndpointContext>() == 0x20);

#[repr(C, align(32))]
#[derive(Default)]
struct DeviceContext {
    slot_ctx: [u32; 8],
    ep_ctx: [EndpointContext; 2 * 15 + 1],
}
const _: () = assert!(size_of::<DeviceContext>() == 0x400);

const _: () = assert!(size_of::<DeviceContext>() == 0x400);
#[repr(C, align(4096))]
#[derive(Default)]
struct OutputContext {
    device_ctx: DeviceContext,
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<OutputContext>() <= 4096);

struct DeviceContextBaseAddressArray {
    inner: Pin<Box<RawDeviceContextBaseAddressArray>>,
    _context: [Option<Pin<Box<OutputContext>>>; 255],
    _scratchpad_buffers: ScratchpadBuffers,
}
impl DeviceContextBaseAddressArray {
    fn new(scratchpad_buffers: ScratchpadBuffers) -> Self {
        let mut inner = RawDeviceContextBaseAddressArray::new();
        inner.scratchpad_table_ptr = scratchpad_buffers.table.as_ref().as_ptr();
        let inner = Box::pin(inner);
        Self {
            inner,
            _context: unsafe { MaybeUninit::zeroed().assume_init() },
            _scratchpad_buffers: scratchpad_buffers,
        }
    }
    fn inner_mut_ptr(&mut self) -> *const RawDeviceContextBaseAddressArray {
        self.inner.as_ref().get_ref() as *const RawDeviceContextBaseAddressArray
    }
}

struct Controller {
    regs: XhcRegisters,
    device_context_base_array: Mutex<DeviceContextBaseAddressArray>,
    primary_event_ring: Mutex<EventRing>,
    command_ring: Mutex<CommandRing>,
}
impl Controller {
    fn new(mut regs: XhcRegisters) -> Result<Self> {
        unsafe {
            regs.op_regs.get_unchecked_mut().reset_xhc();
        }
        let scratchpad_buffers =
            ScratchpadBuffers::alloc(regs.cap_regs.as_ref(), regs.op_regs.as_ref())?;
        let device_context_base_array = DeviceContextBaseAddressArray::new(scratchpad_buffers);
        let device_context_base_array = Mutex::new(device_context_base_array);
        let primary_event_ring = Mutex::new(EventRing::new()?);
        let command_ring = Mutex::new(CommandRing::default());
        let mut xhc = Self {
            regs,
            device_context_base_array,
            primary_event_ring,
            command_ring,
        };
        xhc.init_primary_event_ring()?;
        xhc.init_slots_and_contexts()?;
        xhc.init_command_ring();
        info!("Starting xHC...");
        unsafe { xhc.regs.op_regs.get_unchecked_mut() }.start_xhc();
        info!("xHC started running!");
        Ok(xhc)
    }
    fn init_primary_event_ring(&mut self) -> Result<()> {
        let eq = &mut self.primary_event_ring;
        unsafe { self.regs.rt_regs.get_unchecked_mut() }.init_irs(0, &mut eq.lock())
    }
    fn init_command_ring(&mut self) {
        unsafe { self.regs.op_regs.get_unchecked_mut() }
            .set_cmd_ring_ctrl(&self.command_ring.lock());
    }
    fn init_slots_and_contexts(&mut self) -> Result<()> {
        let num_slots = self.regs.cap_regs.as_ref().num_of_device_slots();
        unsafe { self.regs.op_regs.get_unchecked_mut() }.set_num_device_slots(num_slots)?;
        unsafe { self.regs.op_regs.get_unchecked_mut() }
            .set_dcbaa_ptr(&mut self.device_context_base_array.lock())
    }
}

struct EventRing {
    ring: IoBox<TrbRing>,
    erst: IoBox<EventRingSegmentTableEntry>,
    cycle_state_ours: bool,
    erdp: Option<*mut u64>,
    wait_list: VecDeque<Weak<EventWaitInfo>>,
}
impl EventRing {
    fn new() -> Result<Self> {
        let ring = TrbRing::new();
        let erst = EventRingSegmentTableEntry::new(&ring)?;
        Ok(Self {
            ring,
            erst,
            cycle_state_ours: true,
            erdp: None,
            wait_list: Default::default(),
        })
    }
    fn ring_phys_addr(&self) -> u64 {
        self.ring.as_ref() as *const TrbRing as u64
    }
    fn set_erdp(&mut self, erdp: *mut u64) {
        self.erdp = Some(erdp);
    }
    fn erst_phys_addr(&self) -> u64 {
        self.erst.as_ref() as *const EventRingSegmentTableEntry as u64
    }
    /// Non-blocking
    fn pop(&mut self) -> Result<Option<GenericTrbEntry>> {
        if !self.has_next_event() {
            return Ok(None);
        }
        let e = self.ring.as_ref().current();
        let eptr = self.ring.as_ref().current_ptr() as u64;
        unsafe { self.ring.get_unchecked_mut() }.advance_index_notoggle(self.cycle_state_ours)?;
        unsafe {
            let erdp = self.erdp.expect("erdp is not set");
            write_volatile(erdp, eptr | (*erdp & 0b1111));
        }
        if self.ring.as_ref().current_index() == 0 {
            self.cycle_state_ours = !self.cycle_state_ours;
        }
        Ok(Some(e))
    }
    async fn poll(&mut self) -> Result<()> {
        if let Some(e) = self.pop()? {
            let mut consumed = false;
            for w in &self.wait_list {
                if let Some(w) = w.upgrade() {
                    let w: &EventWaitInfo = w.as_ref();
                    if w.matches(&e) {
                        w.resolve(&e)?;
                        consumed = true;
                    }
                }
            }
            if !consumed {
                info!("unhandled event: {e:?}");
            }
            // cleanup stale waiters
            let stale_waiter_indices = self
                .wait_list
                .iter()
                .enumerate()
                .rev()
                .filter_map(|e| -> Option<usize> {
                    if e.1.strong_count() == 0 {
                        Some(e.0)
                    } else {
                        None
                    }
                })
                .collect::<Vec<usize>>();
            for k in stale_waiter_indices {
                self.wait_list.remove(k);
            }
        }
        Ok(())
    }
    fn has_next_event(&self) -> bool {
        self.ring.as_ref().current().cycle_state() == self.cycle_state_ours
    }
}
#[repr(C, align(4096))]
struct EventRingSegmentTableEntry {
    ring_segment_base_address: u64,
    ring_segment_size: u16,
    _rsvdz: [u16; 3],
}
const _: () = assert!(size_of::<EventRingSegmentTableEntry>() == 4096);
impl EventRingSegmentTableEntry {
    fn new(ring: &IoBox<TrbRing>) -> Result<IoBox<Self>> {
        let mut erst: IoBox<Self> = IoBox::new();
        {
            let erst = unsafe { erst.get_unchecked_mut() };
            erst.ring_segment_base_address = ring.as_ref() as *const TrbRing as u64;
            erst.ring_segment_size = ring
                .as_ref()
                .num_trbs()
                .try_into()
                .or(Err("Too large num trbs"))?;
        }
        Ok(erst)
    }
}
#[repr(C, align(4096))]
struct TrbRing {
    trb: [GenericTrbEntry; Self::NUM_TRB],
    current_index: usize,
    _pinned: PhantomPinned,
}
// Limiting the size of TrbRing to be equal or less than 4096
// to avoid crossing 64KiB boundaries. See Table 6-1 of xhci spec.
const _: () = assert!(size_of::<TrbRing>() <= 4096);
impl TrbRing {
    const NUM_TRB: usize = 16;
    fn new() -> IoBox<Self> {
        IoBox::new()
    }
    const fn num_trbs(&self) -> usize {
        Self::NUM_TRB
    }
    fn write(&mut self, index: usize, trb: GenericTrbEntry) -> Result<()> {
        if index < self.trb.len() {
            unsafe {
                write_volatile(&mut self.trb[index], trb);
            }
            Ok(())
        } else {
            Err("TrbRing Out of Range")
        }
    }
    fn phys_addr(&self) -> u64 {
        &self.trb[0] as *const GenericTrbEntry as u64
    }
    fn current_index(&self) -> usize {
        self.current_index
    }
    fn advance_index_notoggle(&mut self, cycle_ours: bool) -> Result<()> {
        if self.current().cycle_state() != cycle_ours {
            return Err("cycle state mismatch");
        }
        self.current_index = (self.current_index + 1) % self.trb.len();
        Ok(())
    }
    fn current(&self) -> GenericTrbEntry {
        self.trb(self.current_index)
    }
    fn trb(&self, index: usize) -> GenericTrbEntry {
        unsafe { read_volatile(&self.trb[index]) }
    }
    fn current_ptr(&self) -> usize {
        &self.trb[self.current_index] as *const GenericTrbEntry as usize
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(u32)]
#[non_exhaustive]
#[derive(PartialEq, Eq)]
#[allow(unused)]
enum TrbType {
    Normal = 1,
    SetupStage = 2,
    DataStage = 3,
    StatusStage = 4,
    Link = 6,
    EnableSlotCommand = 9,
    AddressDeviceCommand = 11,
    ConfigureEndpointCommand = 12,
    EvaluateContextCommand = 13,
    NoOpCommand = 23,
    TransferEvent = 32,
    CommandCompletionEvent = 33,
    PortStatusChangeEvent = 34,
    HostControllerEvent = 37,
}

#[derive(Default, Clone, Debug)]
#[repr(C, align(16))]
struct GenericTrbEntry {
    data: Volatile<u64>,
    option: Volatile<u32>,
    control: Volatile<u32>,
}
const _: () = assert!(size_of::<GenericTrbEntry>() == 16);
impl GenericTrbEntry {
    fn trb_link(ring: &TrbRing) -> Self {
        let mut trb = GenericTrbEntry::default();
        trb.set_trb_type(TrbType::Link);
        trb.data.write(ring.phys_addr());
        trb.set_toggle_cycle(true);
        trb
    }
    fn set_trb_type(&mut self, trb_type: TrbType) {
        self.control.write_bits(10, 6, trb_type as u32).unwrap()
    }
    fn set_toggle_cycle(&mut self, value: bool) {
        self.control.write_bits(1, 1, value.into()).unwrap()
    }
    fn data(&self) -> u64 {
        self.data.read()
    }
    fn slot_id(&self) -> u8 {
        self.control.read_bits(24, 8).try_into().unwrap()
    }
    fn trb_type(&self) -> u32 {
        self.control.read_bits(10, 6)
    }
    fn cycle_state(&self) -> bool {
        self.control.read_bits(0, 1) != 0
    }
}

struct CommandRing {
    ring: IoBox<TrbRing>,
    _cycle_state_ours: bool,
}
impl CommandRing {
    fn ring_phys_addr(&self) -> u64 {
        self.ring.as_ref() as *const TrbRing as u64
    }
}
impl Default for CommandRing {
    fn default() -> Self {
        let mut this = Self {
            ring: TrbRing::new(),
            _cycle_state_ours: false,
        };
        let link_trb = GenericTrbEntry::trb_link(this.ring.as_ref());
        unsafe { this.ring.get_unchecked_mut() }
            .write(TrbRing::NUM_TRB - 1, link_trb)
            .expect("failed to write a link trb");
        this
    }
}

#[derive(Debug)]
struct EventWaitCond {
    trb_type: Option<TrbType>,
    trb_addr: Option<u64>,
    slot: Option<u8>,
}

#[derive(Debug)]
struct EventWaitInfo {
    cond: EventWaitCond,
    trbs: Mutex<VecDeque<GenericTrbEntry>>,
}
impl EventWaitInfo {
    fn matches(&self, trb: &GenericTrbEntry) -> bool {
        if let Some(trb_type) = self.cond.trb_type {
            if trb.trb_type() != trb_type as u32 {
                return false;
            }
        }
        if let Some(slot) = self.cond.slot {
            if trb.slot_id() != slot {
                return false;
            }
        }
        if let Some(trb_addr) = self.cond.trb_addr {
            if trb.data() != trb_addr {
                return false;
            }
        }
        true
    }
    fn resolve(&self, trb: &GenericTrbEntry) -> Result<()> {
        self.trbs.under_locked(&|trbs| -> Result<()> {
            trbs.push_back(trb.clone());
            Ok(())
        })
    }
}

// Interface to access PORTSC registers
//
// [xhci] 5.4.8: PORTSC
// OperationalBase + (0x400 + 0x10 * (n - 1))
// where n = Port Number (1, 2, ..., MaxPorts)
struct PortSc {
    entries: Vec<Rc<PortScEntry>>,
}
impl PortSc {
    fn new(bar: &BarMem64, cap_regs: &CapabilityRegisters) -> Self {
        let base = unsafe { bar.addr().add(cap_regs.caplength()).add(0x400) } as *mut u32;
        let num_ports = cap_regs.num_of_ports();
        let mut entries = Vec::new();
        for port in 1..=num_ports {
            // SAFETY: This is safe since the result of ptr calculation
            // always points to a valid PORTSC entry under the condition.
            let ptr = unsafe { base.add((port - 1) * 4) };
            entries.push(Rc::new(PortScEntry::new(ptr)));
        }
        assert!(entries.len() == num_ports);
        Self { entries }
    }
    fn port_range(&self) -> Range<usize> {
        1..self.entries.len() + 1
    }
    fn get(&self, port: usize) -> Option<Rc<PortScEntry>> {
        self.entries.get(port.wrapping_sub(1)).cloned()
    }
}
#[repr(C)]
struct PortScEntry {
    ptr: Mutex<*mut u32>,
}
impl PortScEntry {
    fn new(ptr: *mut u32) -> Self {
        Self {
            ptr: Mutex::new(ptr),
        }
    }
    fn value(&self) -> u32 {
        let portsc = self.ptr.lock();
        unsafe { read_volatile(*portsc) }
    }
}
