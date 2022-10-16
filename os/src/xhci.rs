extern crate alloc;

use crate::allocator::ALLOCATOR;
use crate::arch::x86_64::busy_loop_hint;
use crate::arch::x86_64::clflush;
use crate::arch::x86_64::paging::disable_cache;
use crate::arch::x86_64::paging::IoBox;
use crate::arch::x86_64::paging::Mmio;
use crate::error::Result;
use crate::error::WasabiError;
use crate::executor::yield_execution;
use crate::executor::Executor;
use crate::executor::Task;
use crate::hpet::Hpet;
use crate::pci::BarMem64;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::print;
use crate::println;
use crate::usb::ConfigDescriptor;
use crate::usb::DescriptorIterator;
use crate::usb::DescriptorType;
use crate::usb::DeviceDescriptor;
use crate::usb::EndpointDescriptor;
use crate::usb::InterfaceDescriptor;
use crate::usb::IntoPinnedMutableSlice;
use crate::usb::UsbDescriptor;
use crate::util::extract_bits;
use crate::volatile::Volatile;
use alloc::alloc::Layout;
use alloc::boxed::Box;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::cell::SyncUnsafeCell;
use core::cmp::max;
use core::convert::AsRef;
use core::future::Future;
use core::marker::PhantomPinned;
use core::mem::size_of;
use core::mem::transmute;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::ptr::null_mut;
use core::ptr::read_volatile;
use core::ptr::write_volatile;
use core::slice;
use core::task::Context;
use core::task::Poll;

use regs::PortLinkState;
use regs::PortState;

#[derive(Debug, Copy, Clone)]
#[repr(u32)]
#[non_exhaustive]
#[allow(unused)]
#[derive(PartialEq, Eq)]
enum TrbType {
    Normal = 1,
    SetupStage = 2,
    DataStage = 3,
    StatusStage = 4,
    Link = 6,
    EnableSlotCommand = 9,
    AddressDeviceCommand = 11,
    ConfigureEndpointCommand = 12,
    NoOpCommand = 23,
    TransferEvent = 32,
    CommandCompletionEvent = 33,
    PortStatusChangeEvent = 34,
    HostControllerEvent = 37,
}

// 6.4.5 TRB Completion Code
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
#[non_exhaustive]
#[allow(unused)]
#[derive(PartialEq, Eq)]
enum CompletionCode {
    Success = 1,
    UsbTransactionError = 4,
    TrbError = 5,
    StallError = 6,
    ShortPacket = 13,
    ParameterError = 17,
    EventRingFullError = 21,
}
impl CompletionCode {
    fn parse(code: u32) -> &'static str {
        match code {
            code if code == CompletionCode::Success as u32 => "Success",
            code if code == CompletionCode::UsbTransactionError as u32 => "UsbTransactionError",
            code if code == CompletionCode::TrbError as u32 => "TrbError",
            code if code == CompletionCode::StallError as u32 => "StallError",
            code if code == CompletionCode::ShortPacket as u32 => "ShortPacket",
            code if code == CompletionCode::ParameterError as u32 => "ParameterError",
            code if code == CompletionCode::EventRingFullError as u32 => "EventRingFullError",
            _ => "?",
        }
    }
}

#[derive(Copy, Clone, Default)]
#[repr(C, align(16))]
struct GenericTrbEntry {
    data: Volatile<u64>,
    option: Volatile<u32>,
    control: Volatile<u32>,
}
const _: () = assert!(size_of::<GenericTrbEntry>() == 16);
impl GenericTrbEntry {
    const CTRL_BIT_INTERRUPT_ON_SHORT_PACKET: u32 = 1 << 2;
    const CTRL_BIT_INTERRUPT_ON_COMPLETION: u32 = 1 << 5;
    const CTRL_BIT_IMMEDIATE_DATA: u32 = 1 << 6;
    const CTRL_BIT_DATA_DIR_IN: u32 = 1 << 16;
    fn cycle_state(&self) -> bool {
        self.control.read_bits(0, 1) != 0
    }
    fn set_cycle_state(&mut self, cycle: bool) {
        self.control.write_bits(0, 1, cycle.into()).unwrap()
    }
    fn set_toggle_cycle(&mut self, value: bool) {
        self.control.write_bits(1, 1, value.into()).unwrap()
    }
    fn trb_type(&self) -> u32 {
        self.control.read_bits(10, 6)
    }
    fn set_trb_type(&mut self, trb_type: TrbType) {
        self.control.write_bits(10, 6, trb_type as u32).unwrap()
    }
    fn dci(&self) -> usize {
        extract_bits(self.control.read(), 16, 5)
            .try_into()
            .expect("Invalid ep dci")
    }
    fn completion_code(&self) -> u32 {
        self.option.read_bits(24, 8)
    }
    fn slot_id(&self) -> u8 {
        self.control.read_bits(24, 8).try_into().unwrap()
    }
    fn set_slot_id(&mut self, slot: u8) {
        self.control.write_bits(24, 8, slot as u32).unwrap()
    }
    fn completed(&self) -> Result<()> {
        if self.trb_type() != TrbType::CommandCompletionEvent as u32
            && self.trb_type() != TrbType::TransferEvent as u32
        {
            Err(WasabiError::FailedString(format!(
                "Expected TrbType == CommandCompletionEvent or TransferEvent but got {}",
                self.trb_type(),
            )))
        } else if self.completion_code() != CompletionCode::Success as u32
            && self.completion_code() != CompletionCode::ShortPacket as u32
        {
            Err(WasabiError::FailedString(format!(
                "Expected CompletionCode == Success but got {} ({})",
                self.completion_code() as u32,
                CompletionCode::parse(self.completion_code())
            )))
        } else {
            Ok(())
        }
    }
    fn cmd_no_op() -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::NoOpCommand);
        trb
    }
    fn cmd_enable_slot() -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::EnableSlotCommand);
        trb
    }
    fn cmd_address_device(input_context: Pin<&InputContext>, slot_id: u8) -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::AddressDeviceCommand);
        trb.data
            .write(input_context.get_ref() as *const InputContext as u64);
        trb.set_slot_id(slot_id);
        trb
    }
    fn cmd_configure_endpoint(input_context: Pin<&InputContext>, slot_id: u8) -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::ConfigureEndpointCommand);
        trb.data
            .write(input_context.get_ref() as *const InputContext as u64);
        trb.set_slot_id(slot_id);
        trb
    }
    fn trb_link(ring: &TrbRing) -> Self {
        let mut trb = GenericTrbEntry::default();
        trb.set_trb_type(TrbType::Link);
        trb.data.write(ring.phys_addr());
        trb.set_toggle_cycle(true);
        trb
    }
}
impl Debug for GenericTrbEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.trb_type() {
            e if e == (TrbType::CommandCompletionEvent as u32) => {
                write!(
                    f,
                    "CommandCompletionEvent CompletionCode = {} ({}) for {:#018X}",
                    self.completion_code(),
                    CompletionCode::parse(self.completion_code()),
                    self.data.read()
                )
            }
            e if e == (TrbType::TransferEvent as u32) => {
                write!(
                    f,
                    "TransferEvent CompletionCode = {} ({}), data = {:#018X}, slot = {}, dci = {}",
                    self.completion_code(),
                    CompletionCode::parse(self.completion_code()),
                    self.data.read(),
                    self.slot_id(),
                    self.dci()
                )
            }
            e if e == (TrbType::HostControllerEvent as u32) => {
                write!(
                    f,
                    "HostControllerEvent CompletionCode = {} ({})",
                    self.completion_code(),
                    CompletionCode::parse(self.completion_code())
                )
            }
            e if e == (TrbType::PortStatusChangeEvent as u32) => {
                write!(f, "PortStatusChangeEvent Port = {}", self.data.read() >> 24,)
            }
            _ => {
                write!(f, "TRB type={:?}", self.trb_type())
            }
        }
    }
}
// Following From<*Trb> impls are safe
// since GenericTrbEntry generated from any TRB will be valid.
impl From<SetupStageTrb> for GenericTrbEntry {
    fn from(trb: SetupStageTrb) -> GenericTrbEntry {
        unsafe { transmute(trb) }
    }
}
impl From<DataStageTrb> for GenericTrbEntry {
    fn from(trb: DataStageTrb) -> GenericTrbEntry {
        unsafe { transmute(trb) }
    }
}
impl From<StatusStageTrb> for GenericTrbEntry {
    fn from(trb: StatusStageTrb) -> GenericTrbEntry {
        unsafe { transmute(trb) }
    }
}
impl From<NormalTrb> for GenericTrbEntry {
    fn from(trb: NormalTrb) -> GenericTrbEntry {
        unsafe { transmute(trb) }
    }
}

#[derive(Copy, Clone)]
#[repr(C, align(16))]
struct NormalTrb {
    buf: u64,
    option: u32,
    control: u32,
}
const _: () = assert!(size_of::<DataStageTrb>() == 16);
impl NormalTrb {
    const CONTROL_INTERRUPT_ON_COMPLETION: u32 = 1 << 5;
    const CONTROL_INTERRUPT_ON_SHORT_PACKET: u32 = 1 << 2;
    fn new(buf: *mut u8, size: u16) -> Self {
        Self {
            buf: buf as u64,
            option: size as u32,
            control: (TrbType::Normal as u32) << 10
                | Self::CONTROL_INTERRUPT_ON_COMPLETION
                | Self::CONTROL_INTERRUPT_ON_SHORT_PACKET,
        }
    }
}

#[derive(Copy, Clone)]
#[repr(C, align(16))]
struct SetupStageTrb {
    // [xHCI] 6.4.1.2.1 Setup Stage TRB
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
    option: u32,
    control: u32,
}
const _: () = assert!(size_of::<SetupStageTrb>() == 16);
impl SetupStageTrb {
    // bmRequest bit[7]: Data Transfer Direction
    //      0: Host to Device
    //      1: Device to Host
    const REQ_TYPE_DIR_DEVICE_TO_HOST: u8 = 1 << 7;
    //const REQ_TYPE_DIR_HOST_TO_DEVICE: u8 = 0 << 7;
    // bmRequest bit[5..=6]: Request Type
    //      0: Standard
    //      1: Class
    //      2: Vendor
    //      _: Reserved
    //const REQ_TYPE_TYPE_STANDARD: u8 = 0 << 5;
    const REQ_TYPE_TYPE_CLASS: u8 = 1 << 5;
    //const REQ_TYPE_TYPE_VENDOR: u8 = 2 << 5;
    // bmRequest bit[0..=4]: Recipient
    //      0: Device
    //      1: Interface
    //      2: Endpoint
    //      3: Other
    //      _: Reserved
    //const REQ_TYPE_TO_DEVICE: u8 = 0;
    const REQ_TYPE_TO_INTERFACE: u8 = 1;
    //const REQ_TYPE_TO_ENDPOINT: u8 = 2;
    //const REQ_TYPE_TO_OTHER: u8 = 3;

    const REQ_GET_REPORT: u8 = 1;
    const REQ_GET_DESCRIPTOR: u8 = 6;
    const REQ_SET_CONFIGURATION: u8 = 9;
    const REQ_SET_INTERFACE: u8 = 11;
    const REQ_SET_PROTOCOL: u8 = 0x0b;
    fn new(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Self {
        // Table 4-7: USB SETUP Data to Data Stage TRB and Status Stage TRB mapping
        const TRT_NO_DATA_STAGE: u32 = 0;
        const TRT_OUT_DATA_STAGE: u32 = 2;
        const TRT_IN_DATA_STAGE: u32 = 3;
        let transfer_type = if length == 0 {
            TRT_NO_DATA_STAGE
        } else if request & Self::REQ_TYPE_DIR_DEVICE_TO_HOST != 0 {
            TRT_IN_DATA_STAGE
        } else {
            TRT_OUT_DATA_STAGE
        };
        Self {
            request_type,
            request,
            value,
            index,
            length,
            option: 8,
            control: transfer_type << 16
                | (TrbType::SetupStage as u32) << 10
                | GenericTrbEntry::CTRL_BIT_IMMEDIATE_DATA,
        }
    }
}

#[derive(Copy, Clone)]
#[repr(C, align(16))]
struct DataStageTrb {
    buf: u64,
    option: u32,
    control: u32,
}
const _: () = assert!(size_of::<DataStageTrb>() == 16);
impl DataStageTrb {
    fn new_in(buf: Pin<&mut [u8]>) -> Self {
        Self {
            buf: buf.as_ptr() as u64,
            option: buf.len() as u32,
            control: (TrbType::DataStage as u32) << 10
                | GenericTrbEntry::CTRL_BIT_DATA_DIR_IN
                | GenericTrbEntry::CTRL_BIT_INTERRUPT_ON_COMPLETION
                | GenericTrbEntry::CTRL_BIT_INTERRUPT_ON_SHORT_PACKET,
        }
    }
}

// Status stage direction will be opposite of the data.
// If there is no data transfer, status direction should be "in".
// See Table 4-7 of xHCI spec.
#[derive(Copy, Clone)]
#[repr(C, align(16))]
struct StatusStageTrb {
    reserved: u64,
    option: u32,
    control: u32,
}
const _: () = assert!(size_of::<StatusStageTrb>() == 16);
impl StatusStageTrb {
    fn new_out() -> Self {
        Self {
            reserved: 0,
            option: 0,
            control: (TrbType::StatusStage as u32) << 10,
        }
    }
    fn new_in() -> Self {
        Self {
            reserved: 0,
            option: 0,
            control: (TrbType::StatusStage as u32) << 10
                | GenericTrbEntry::CTRL_BIT_DATA_DIR_IN
                | GenericTrbEntry::CTRL_BIT_INTERRUPT_ON_COMPLETION
                | GenericTrbEntry::CTRL_BIT_INTERRUPT_ON_SHORT_PACKET,
        }
    }
}

fn error_stringify<T: Debug>(x: T) -> String {
    format!("[xHC] Error: {x:?}")
}

#[derive(Debug)]
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
    fn phys_addr(&self) -> u64 {
        &self.trb[0] as *const GenericTrbEntry as u64
    }
    const fn num_trbs(&self) -> usize {
        Self::NUM_TRB
    }
    fn advance_index(&mut self, new_cycle: bool) -> Result<()> {
        if self.current().cycle_state() == new_cycle {
            return Err(WasabiError::Failed("cycle state does not change"));
        }
        self.trb[self.current_index].set_cycle_state(new_cycle);
        self.current_index = (self.current_index + 1) % self.trb.len();
        Ok(())
    }
    fn advance_index_notoggle(&mut self, cycle_ours: bool) -> Result<()> {
        if self.current().cycle_state() != cycle_ours {
            return Err(WasabiError::Failed("cycle state mismatch"));
        }
        self.current_index = (self.current_index + 1) % self.trb.len();
        Ok(())
    }
    fn current(&self) -> GenericTrbEntry {
        self.trb(self.current_index)
    }
    fn current_index(&self) -> usize {
        self.current_index
    }
    fn current_ptr(&self) -> usize {
        &self.trb[self.current_index] as *const GenericTrbEntry as usize
    }
    fn trb(&self, index: usize) -> GenericTrbEntry {
        unsafe { read_volatile(&self.trb[index]) }
    }
    fn trb_ptr(&self, index: usize) -> usize {
        &self.trb[index] as *const GenericTrbEntry as usize
    }
    fn write(&mut self, index: usize, trb: GenericTrbEntry) -> Result<()> {
        if index < self.trb.len() {
            unsafe {
                write_volatile(&mut self.trb[index], trb);
            }
            clflush(&self.trb[index] as *const GenericTrbEntry as usize);
            Ok(())
        } else {
            Err(WasabiError::Failed("TrbRing Out of Range"))
        }
    }
    fn write_current(&mut self, trb: GenericTrbEntry) {
        self.write(self.current_index, trb)
            .expect("writing to the current index shall not fail")
    }
}

struct CommandRing {
    ring: IoBox<TrbRing>,
    cycle_state_ours: bool,
}
impl Default for CommandRing {
    fn default() -> Self {
        let mut this = Self {
            ring: TrbRing::new(),
            cycle_state_ours: false,
        };
        let link_trb = GenericTrbEntry::trb_link(this.ring.as_ref());
        unsafe { this.ring.get_unchecked_mut() }
            .write(TrbRing::NUM_TRB - 1, link_trb)
            .expect("failed to write a link trb");
        this
    }
}
impl CommandRing {
    fn ring_phys_addr(&self) -> u64 {
        self.ring.as_ref() as *const TrbRing as u64
    }
    fn push(&mut self, mut src: GenericTrbEntry) -> Result<u64> {
        // Calling get_unchecked_mut() here is safe
        // as far as this function does not move the ring out.
        let ring = unsafe { self.ring.get_unchecked_mut() };
        if ring.current().cycle_state() != self.cycle_state_ours {
            return Err(WasabiError::Failed("Command Ring is Full"));
        }
        src.set_cycle_state(self.cycle_state_ours);
        let dst_ptr = ring.current_ptr();
        ring.write_current(src);
        ring.advance_index(!self.cycle_state_ours)?;
        if ring.current().trb_type() == TrbType::Link as u32 {
            // Reached to Link TRB. Let's skip it and toggle the cycle.
            ring.advance_index(!self.cycle_state_ours)?;
            self.cycle_state_ours = !self.cycle_state_ours;
        }
        // The returned ptr will be used for waiting on command completion events.
        Ok(dst_ptr as u64)
    }
}

// Producer: Software
// Consumer: xHC
// Producer is responsible to flip the cycle bits
struct TransferRing {
    ring: IoBox<TrbRing>,
    cycle_state_ours: bool,
    // enqeue_index: usize, // will be maintained by .ring
    dequeue_index: usize,
    buffers: [*mut u8; TrbRing::NUM_TRB - 1],
}
impl TransferRing {
    const BUF_SIZE: usize = 4096;
    const BUF_ALIGN: usize = 4096;
    fn new() -> Result<Self> {
        let mut this = Self {
            ring: TrbRing::new(),
            cycle_state_ours: false,
            dequeue_index: 0,
            buffers: [null_mut(); TrbRing::NUM_TRB - 1],
        };
        // Fill all TRBs but keep them owned by us
        let link_trb = GenericTrbEntry::trb_link(this.ring.as_ref());
        let num_trbs = this.ring.as_ref().num_trbs();
        let mut_ring = unsafe { this.ring.get_unchecked_mut() };
        mut_ring
            .write(num_trbs - 1, link_trb)
            .expect("failed to write a link trb");
        for (i, v) in this.buffers.iter_mut().enumerate() {
            *v = ALLOCATOR.alloc_with_options(
                Layout::from_size_align(Self::BUF_SIZE, Self::BUF_ALIGN)
                    .map_err(error_stringify)?,
            );
            mut_ring
                .write(i, NormalTrb::new(*v, 8).into())
                .expect("failed to write a link trb");
        }
        Ok(this)
    }
    fn fill_ring(&mut self) -> Result<()> {
        // 4.9.2.2 Pointer Advancement
        // To prevent overruns, software shall determine when the Ring is full. The ring is
        // defined as “full” if advancing the Enqueue Pointer will make it equal to the
        // Dequeue Pointer.
        loop {
            // Wrap with num_trbs() - 1 to ignore LinkTrb
            let next_enqueue_index =
                (self.ring.as_ref().current_index() + 1) % (self.ring.as_ref().num_trbs() - 1);
            if next_enqueue_index == self.dequeue_index {
                // Ring is full. stop filling.
                break;
            }
            let mut_ring = unsafe { self.ring.get_unchecked_mut() };
            mut_ring.advance_index(!self.cycle_state_ours)?;
        }
        Ok(())
    }
    fn dequeue_trb(&mut self, trb_ptr: usize) -> Result<()> {
        println!("releasing TRB @ {:#018X}", trb_ptr);
        // Update dequeue_index
        if self.ring.as_ref().trb_ptr(self.dequeue_index) != trb_ptr {
            return Err(WasabiError::Failed("unexpected trb ptr"));
        }
        // Wrap with num_trbs() - 1 to ignore LinkTrb
        self.dequeue_index = (self.dequeue_index + 1) % (self.ring.as_ref().num_trbs() - 1);
        // Update enqeue_index
        let mut_ring = unsafe { self.ring.get_unchecked_mut() };
        mut_ring.advance_index(!self.cycle_state_ours)?;
        if mut_ring.current().trb_type() == TrbType::Link as u32 {
            // Reached to Link TRB. Let's skip it and toggle the cycle.
            mut_ring.advance_index(!self.cycle_state_ours)?;
            self.cycle_state_ours = !self.cycle_state_ours;
        }
        Ok(())
    }
    fn ring_phys_addr(&self) -> u64 {
        self.ring.as_ref() as *const TrbRing as u64
    }
}

struct EventRing {
    ring: IoBox<TrbRing>,
    erst: IoBox<EventRingSegmentTableEntry>,
    cycle_state_ours: bool,
    erdp: Option<*mut u64>,
}
impl EventRing {
    fn new() -> Result<Self> {
        let ring = TrbRing::new();
        let erst = EventRingSegmentTableEntry::new(&ring)?;
        disable_cache(&erst);

        Ok(Self {
            ring,
            erst,
            cycle_state_ours: true,
            erdp: None,
        })
    }
    fn set_erdp(&mut self, erdp: *mut u64) {
        self.erdp = Some(erdp);
    }
    fn ring_phys_addr(&self) -> u64 {
        self.ring.as_ref() as *const TrbRing as u64
    }
    fn erst_phys_addr(&self) -> u64 {
        self.erst.as_ref() as *const EventRingSegmentTableEntry as u64
    }
    fn has_next_event(&self) -> bool {
        self.ring.as_ref().current().cycle_state() == self.cycle_state_ours
    }
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
}

enum EventFutureWaitType {
    TrbAddr(u64),
    Slot(u8),
}

struct EventFuture<'a, const E: TrbType> {
    event_ring: &'a mut EventRing,
    wait_on: EventFutureWaitType,
    time_out: u64,
    _pinned: PhantomPinned,
}
impl<'a, const E: TrbType> EventFuture<'a, E> {
    fn new(event_ring: &'a mut EventRing, trb_addr: u64) -> Self {
        let time_out = Hpet::take().main_counter() + Hpet::take().freq() / 10;
        Self {
            event_ring,
            wait_on: EventFutureWaitType::TrbAddr(trb_addr),
            time_out,
            _pinned: PhantomPinned,
        }
    }
    fn new_on_slot(event_ring: &'a mut EventRing, slot: u8) -> Self {
        let time_out = Hpet::take().main_counter() + Hpet::take().freq() / 10;
        Self {
            event_ring,
            wait_on: EventFutureWaitType::Slot(slot),
            time_out,
            _pinned: PhantomPinned,
        }
    }
}
impl<'a, const E: TrbType> Future for EventFuture<'a, E> {
    type Output = Result<Option<GenericTrbEntry>>;
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<Option<GenericTrbEntry>>> {
        let time_out = self.time_out;
        let mut_self = unsafe { self.get_unchecked_mut() };
        match mut_self.event_ring.pop() {
            Err(e) => Poll::Ready(Err(e)),
            Ok(None) => {
                if time_out < Hpet::take().main_counter() {
                    Poll::Ready(Ok(None))
                } else {
                    Poll::Pending
                }
            }
            Ok(Some(trb)) => {
                if trb.trb_type() != E as u32 {
                    println!("Ignoring event (!= type): {:?}", trb);
                    return Poll::Pending;
                }
                match mut_self.wait_on {
                    EventFutureWaitType::TrbAddr(trb_addr) if trb_addr == trb.data.read() => {
                        Poll::Ready(Ok(Some(trb)))
                    }
                    EventFutureWaitType::Slot(slot) if trb.slot_id() == slot => {
                        Poll::Ready(Ok(Some(trb)))
                    }
                    _ => {
                        println!("Ignoring event (!= wait_on): {:?}", trb);
                        Poll::Pending
                    }
                }
            }
        }
    }
}
type CommandCompletionEventFuture<'a> = EventFuture<'a, { TrbType::CommandCompletionEvent }>;
type TransferEventFuture<'a> = EventFuture<'a, { TrbType::TransferEvent }>;

#[repr(C, align(64))]
struct RawDeviceContextBaseAddressArray {
    context: [u64; 256],
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<RawDeviceContextBaseAddressArray>() == 2048);
impl RawDeviceContextBaseAddressArray {
    fn new() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

struct DeviceContextBaseAddressArray {
    inner: Pin<Box<RawDeviceContextBaseAddressArray>>,
    _scratchpad_buffers: Pin<Box<[*mut u8]>>,
}
impl DeviceContextBaseAddressArray {
    fn new(scratchpad_buffers: Pin<Box<[*mut u8]>>) -> Self {
        let mut inner = RawDeviceContextBaseAddressArray::new();
        inner.context[0] = scratchpad_buffers.as_ptr() as u64;
        Self {
            inner: Box::pin(inner),
            _scratchpad_buffers: scratchpad_buffers,
        }
    }
    unsafe fn inner_mut_ptr(&mut self) -> *mut RawDeviceContextBaseAddressArray {
        self.inner.as_mut().get_unchecked_mut() as *mut RawDeviceContextBaseAddressArray
    }
    fn set_output_context(&mut self, slot: u8, output_context: Pin<&mut OutputContext>) {
        unsafe {
            self.inner.as_mut().get_unchecked_mut().context[slot as usize] =
                output_context.as_ref().get_ref() as *const OutputContext as u64
        }
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
            erst.ring_segment_size = ring.as_ref().num_trbs().try_into()?;
        }
        Ok(erst)
    }
}

#[allow(dead_code)]
#[derive(Copy, Clone)]
#[repr(C)]
pub struct CapabilityRegisters {
    length: Volatile<u8>,
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
    pub fn num_of_device_slots(&self) -> usize {
        extract_bits(self.hcsparams1.read(), 0, 8) as usize
    }
    pub fn num_of_interrupters(&self) -> usize {
        extract_bits(self.hcsparams1.read(), 8, 11) as usize
    }
    pub fn num_of_ports(&self) -> usize {
        extract_bits(self.hcsparams1.read(), 24, 8) as usize
    }
    pub fn num_scratch_pad_bufs(&self) -> usize {
        (extract_bits(self.hcsparams2.read(), 21, 5) << 5
            | extract_bits(self.hcsparams2.read(), 27, 5)) as usize
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
const _: () = assert!(size_of::<OperationalRegisters>() == 0x40);
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
const _: () = assert!(size_of::<InterrupterRegisterSet>() == 0x20);
#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
#[repr(C)]
struct RuntimeRegisters {
    rsvdz: [u32; 8],
    irs: [InterrupterRegisterSet; 1024],
}
const _: () = assert!(size_of::<RuntimeRegisters>() == 0x8020);

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
    fn attach(
        &self,
        bdf: BusDeviceFunction,
        executor: Rc<SyncUnsafeCell<Executor>>,
    ) -> Result<Box<dyn PciDeviceDriverInstance>> {
        Ok(Box::new(XhciDriverInstance::new(bdf, executor)?) as Box<dyn PciDeviceDriverInstance>)
    }
    fn name(&self) -> &str {
        "XhciDriver"
    }
}

mod regs {
    use super::busy_loop_hint;
    use super::extract_bits;
    use super::fmt;
    use super::format;
    use super::println;
    use super::read_volatile;
    use super::transmute;
    use super::write_volatile;
    use super::BarMem64;
    use super::CapabilityRegisters;
    use super::Debug;
    use super::Result;
    use super::UsbMode;
    use super::WasabiError;

    #[repr(u32)]
    #[non_exhaustive]
    #[derive(Debug)]
    #[allow(unused)]
    pub enum PortLinkState {
        U0 = 0,
        U1,
        U2,
        U3,
        Disabled,
        RxDetect,
        Inactive,
        Polling,
        Recovery,
        HotReset,
        ComplianceMode,
        TestMode,
        Resume = 15,
    }
    #[repr(C)]
    pub struct PortScWrapper {
        ptr: *mut u32,
    }
    #[derive(Debug)]
    pub enum PortState {
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
        pub fn pls(&self) -> PortLinkState {
            // PLS - Port Link Status - RWS
            unsafe { transmute(extract_bits(self.value(), 5, 4)) }
        }
        pub fn pp(&self) -> bool {
            // PP - Port Power - RWS
            self.value() & Self::BIT_PORT_POWER != 0
        }
        pub fn port_speed(&self) -> UsbMode {
            // Port Speed - ROS
            // Returns Protocol Speed ID (PSI). See 7.2.1 of xhci spec.
            // Default mapping is in Table 7-13: Default USB Speed ID Mapping.
            match extract_bits(self.value(), 10, 4) {
                1 => UsbMode::FullSpeed,
                2 => UsbMode::LowSpeed,
                3 => UsbMode::HighSpeed,
                4 => UsbMode::SuperSpeed,
                v => UsbMode::Unknown(v),
            }
        }
        pub fn max_packet_size(&self) -> Result<u16> {
            match self.port_speed() {
                UsbMode::FullSpeed | UsbMode::LowSpeed => Ok(8),
                UsbMode::HighSpeed => Ok(64),
                UsbMode::SuperSpeed => Ok(512),
                speed => Err(WasabiError::FailedString(format!(
                    "Unknown Protocol Speeed ID: {:?}",
                    speed
                ))),
            }
        }
        pub fn state(&self) -> PortState {
            // 4.19.1.1 USB2 Root Hub Port
            match (self.pp(), self.ccs(), self.ped(), self.pr()) {
                (false, false, false, false) => PortState::PoweredOff,
                (true, false, false, false) => PortState::Disconnected,
                (true, true, false, false) => PortState::Disabled,
                (true, true, false, true) => PortState::Reset,
                (true, true, true, false) => PortState::Enabled,
                tuple => {
                    let (pp, ccs, ped, pr) = tuple;
                    PortState::Other { pp, ccs, ped, pr }
                }
            }
        }
    }
    impl Debug for PortScWrapper {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(
                f,
                "PORTSC: value={:#010X}, state={:?}, link_state={:?}, mode={:?}",
                self.value(),
                self.state(),
                self.pls(),
                self.port_speed(),
            )
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
            let port = self.next_index + 1;
            let portsc = self.list.get(port).ok()?;
            self.next_index += 1;
            Some(PortScIteratorItem { port, portsc })
        }
    }
    // Interface to access PORTSC registers
    pub struct PortSc {
        base: *mut u32,
        num_ports: usize,
    }
    impl PortSc {
        pub fn new(bar: &BarMem64, cap_regs: &CapabilityRegisters) -> Self {
            let base =
                unsafe { bar.addr().add(cap_regs.length.read() as usize).add(0x400) } as *mut u32;
            let num_ports = cap_regs.num_of_ports();
            println!("PORTSC @ {:p}, max_port_num = {}", base, num_ports);
            Self { base, num_ports }
        }
        pub fn get(&self, port: usize) -> Result<PortScWrapper> {
            // [xhci] 5.4.8: PORTSC
            // OperationalBase + (0x400 + 0x10 * (n - 1))
            // where n = Port Number (1, 2, ..., MaxPorts)
            if (1..=self.num_ports).contains(&port) {
                Ok(PortScWrapper {
                    ptr: unsafe { self.base.add((port - 1) * 4) },
                })
            } else {
                Err("xHC: Port Number Out of Range".into())
            }
        }
        pub fn iter(&self) -> PortScIterator {
            PortScIterator {
                list: self,
                next_index: 0,
            }
        }
    }
}

// EndpointType: 0-7
#[derive(Debug, Copy, Clone)]
#[repr(u8)]
#[allow(unused)]
#[derive(PartialEq, Eq)]
enum EndpointType {
    IsochOut = 1,
    BulkOut = 2,
    InterruptOut = 3,
    Control = 4,
    IsochIn = 5,
    BulkIn = 6,
    InterruptIn = 7,
}
impl From<&EndpointDescriptor> for EndpointType {
    fn from(ep_desc: &EndpointDescriptor) -> Self {
        match (ep_desc.endpoint_address >> 7, ep_desc.attributes & 3) {
            (0, 1) => Self::IsochOut,
            (0, 2) => Self::BulkOut,
            (0, 3) => Self::InterruptOut,
            (_, 0) => Self::Control,
            (1, 1) => Self::IsochIn,
            (1, 2) => Self::BulkIn,
            (1, 3) => Self::InterruptIn,
            _ => unreachable!(),
        }
    }
}

#[allow(unused)]
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum UsbMode {
    Unknown(u32),
    FullSpeed,
    LowSpeed,
    HighSpeed,
    SuperSpeed,
}
impl UsbMode {
    fn psi(&self) -> u32 {
        match *self {
            Self::FullSpeed => 1,
            Self::LowSpeed => 2,
            Self::HighSpeed => 3,
            Self::SuperSpeed => 4,
            Self::Unknown(psi) => psi,
        }
    }
}

#[repr(C, align(32))]
#[derive(Default, Debug)]
struct EndpointContext {
    // data[0]:
    //   - bit[16..=23]: Interval (Table 6-12: Endpoint Type vs. Interval Calculation)
    // data[1]:
    //   - bit[1..=2]: Error Count (CErr)
    //   - bit[3..=5]: EndpointType (EPType)
    //   - bit[16..=31]: Max Packet Size (taken from EndpointDescriptor)
    data: [u32; 2],

    tr_dequeue_ptr: Volatile<u64>,

    // 4.14.1.1 (should be non-zero)
    average_trb_length: u16,

    // 4.14.2
    max_esit_payload_low: u16,
    _reserved: [u32; 3],
}
const _: () = assert!(size_of::<EndpointContext>() == 0x20);
impl EndpointContext {
    unsafe fn new() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
    fn new_interrupt_in_endpoint(
        max_packet_size: u16,
        tr_dequeue_ptr: u64,
        mode: UsbMode,
        interval_from_ep_desc: u8,
        average_trb_length: u16,
    ) -> Result<Self> {
        // See [xhci] Table 6-12
        let interval = match mode {
            UsbMode::LowSpeed | UsbMode::FullSpeed => {
                // interval_ms == 2^interval / 8 == interval_from_ep_desc
                interval_from_ep_desc.ilog2() as u8 + 3
            }
            UsbMode::HighSpeed | UsbMode::SuperSpeed => interval_from_ep_desc - 1,
            mode => {
                return Err(WasabiError::FailedString(format!(
                    "Failed to calc interval for {:?}",
                    mode
                )))
            }
        };
        let mut ep = unsafe { Self::new() };
        ep.set_ep_type(EndpointType::InterruptIn)?;
        ep.set_dequeue_cycle_state(true)?;
        ep.set_error_count(3)?;
        ep.set_max_packet_size(max_packet_size);
        ep.set_ring_dequeue_pointer(tr_dequeue_ptr)?;
        ep.data[0] = (interval as u32) << 16;
        ep.average_trb_length = average_trb_length;
        ep.max_esit_payload_low = max_packet_size; // 4.14.2
        println!("{:?}", ep);
        Ok(ep)
    }
    fn new_control_endpoint(max_packet_size: u16, tr_dequeue_ptr: u64) -> Result<Self> {
        let mut ep = unsafe { Self::new() };
        ep.set_ep_type(EndpointType::Control)?;
        ep.set_dequeue_cycle_state(true)?;
        ep.set_error_count(3)?;
        ep.set_max_packet_size(max_packet_size);
        ep.set_ring_dequeue_pointer(tr_dequeue_ptr)?;
        ep.average_trb_length = 8; // 6.2.3: Software shall set Average TRB Length to ‘8’ for control endpoints.
        Ok(ep)
    }
    fn set_ep_type(&mut self, ep_type: EndpointType) -> Result<()> {
        let raw_ep_type = ep_type as u32;
        if raw_ep_type < 8 {
            self.data[1] &= !(0b111 << 3);
            self.data[1] |= raw_ep_type << 3;
            Ok(())
        } else {
            Err(WasabiError::FailedString(format!(
                "Invalid ep_type = {}",
                raw_ep_type
            )))
        }
    }
    fn set_ring_dequeue_pointer(&mut self, tr_dequeue_ptr: u64) -> Result<()> {
        self.tr_dequeue_ptr.write_bits(4, 60, tr_dequeue_ptr >> 4)
    }
    fn set_dequeue_cycle_state(&mut self, dcs: bool) -> Result<()> {
        self.tr_dequeue_ptr.write_bits(0, 1, dcs.into())
    }
    fn set_error_count(&mut self, error_count: u32) -> Result<()> {
        if error_count & !0b11 == 0 {
            self.data[1] &= !(0b11 << 1);
            self.data[1] |= error_count << 1;
            Ok(())
        } else {
            Err(WasabiError::Failed("invalid error_count"))
        }
    }
    fn set_max_packet_size(&mut self, max_packet_size: u16) {
        let max_packet_size = max_packet_size as u32;
        self.data[1] &= !(0xffff << 16);
        self.data[1] |= max_packet_size << 16;
    }
}

#[repr(C, align(32))]
#[derive(Default)]
struct DeviceContext {
    slot_ctx: [u32; 8],
    ep_ctx: [EndpointContext; 2 * 15 + 1],
}
const _: () = assert!(size_of::<DeviceContext>() == 0x400);
impl DeviceContext {
    fn set_root_hub_port_number(&mut self, port: usize) -> Result<()> {
        if 0 < port && port < 256 {
            self.slot_ctx[1] &= !(0xFF << 16);
            self.slot_ctx[1] |= (port as u32) << 16;
            Ok(())
        } else {
            Err(WasabiError::Failed("port out of range"))
        }
    }
    fn set_last_valid_dci(&mut self, dci: usize) -> Result<()> {
        // - 6.2.2:
        // ...the index (dci) of the last valid Endpoint Context
        // This field indicates the size of the Device Context structure.
        // For example, ((Context Entries+1) * 32 bytes) = Total bytes for this structure.
        // - 6.2.2.2:
        // A 'valid' Input Slot Context for a Configure Endpoint Command
        // requires the Context Entries field to be initialized to
        // the index of the last valid Endpoint Context that is
        // defined by the target configuration
        if dci <= 31 {
            self.slot_ctx[0] &= !(0b11111 << 27);
            self.slot_ctx[0] |= (dci as u32) << 27;
            Ok(())
        } else {
            Err(WasabiError::Failed("num_ep_ctx out of range"))
        }
    }
    fn set_port_speed(&mut self, mode: UsbMode) -> Result<()> {
        if mode.psi() < 16u32 {
            self.slot_ctx[0] &= !(0xF << 20);
            self.slot_ctx[0] |= (mode.psi()) << 20;
            Ok(())
        } else {
            Err(WasabiError::Failed("psi out of range"))
        }
    }
}

#[repr(C, align(32))]
#[derive(Default)]
struct InputControlContext {
    drop_context_bitmap: u32,
    add_context_bitmap: u32,
    data: [u32; 6],
}
const _: () = assert!(size_of::<InputControlContext>() == 0x20);
impl InputControlContext {
    fn add_context(&mut self, ici: usize) -> Result<()> {
        if ici < 32 {
            self.add_context_bitmap |= 1 << ici;
            Ok(())
        } else {
            Err(WasabiError::Failed("add_context: ici out of range"))
        }
    }
}

#[repr(C, align(4096))]
#[derive(Default)]
struct InputContext {
    input_ctrl_ctx: InputControlContext,
    device_ctx: DeviceContext,
    //
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<InputContext>() <= 4096);
impl InputContext {
    fn set_root_hub_port_number(self: &mut Pin<&mut Self>, port: usize) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut() }
            .device_ctx
            .set_root_hub_port_number(port)
    }
    fn set_last_valid_dci(self: &mut Pin<&mut Self>, dci: usize) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut() }
            .device_ctx
            .set_last_valid_dci(dci)
    }
    fn set_port_speed(self: &mut Pin<&mut Self>, psi: UsbMode) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut() }
            .device_ctx
            .set_port_speed(psi)
    }
    /// # Arguments
    /// * `dci` - destination device context index. [slot_ctx, ctrl_ep, ep1_out, ep1_in, ...]
    fn set_ep_ctx(self: &mut Pin<&mut Self>, dci: usize, ep_ctx: EndpointContext) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut().device_ctx.ep_ctx[dci - 1] = ep_ctx }
        Ok(())
    }
    fn set_input_ctrl_ctx(
        self: &mut Pin<&mut Self>,
        input_ctrl_ctx: InputControlContext,
    ) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut().input_ctrl_ctx = input_ctrl_ctx }
        Ok(())
    }
}

#[repr(C, align(4096))]
#[derive(Default)]
struct OutputContext {
    device_ctx: DeviceContext,
    //
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<OutputContext>() <= 4096);

enum PollStatus {
    WaitingSomething,
    EnablingPort { port: usize },
    USB3Attached { port: usize },
}

struct SlotContext {
    input_context: Pin<Box<InputContext>>,
    output_context: Pin<Box<OutputContext>>,
    device_descriptor: Pin<Box<DeviceDescriptor>>,
    ctrl_ep_ring: CommandRing,
}
impl SlotContext {
    fn device_descriptor(&mut self) -> Pin<&mut DeviceDescriptor> {
        self.device_descriptor.as_mut()
    }
    fn output_context(&mut self) -> Pin<&mut OutputContext> {
        self.output_context.as_mut()
    }
}
impl Default for SlotContext {
    fn default() -> Self {
        Self {
            input_context: Box::pin(InputContext::default()),
            output_context: Box::pin(OutputContext::default()),
            device_descriptor: Box::pin(DeviceDescriptor::default()),
            ctrl_ep_ring: CommandRing::default(),
        }
    }
}

pub struct Xhci {
    #[allow(dead_code)]
    bdf: BusDeviceFunction,
    cap_regs: Mmio<CapabilityRegisters>,
    op_regs: Mmio<OperationalRegisters>,
    rt_regs: Mmio<RuntimeRegisters>,
    portsc: regs::PortSc,
    doorbell_regs: Mmio<[u32; 256]>,
    command_ring: CommandRing,
    primary_event_ring: EventRing,
    device_context_base_array: DeviceContextBaseAddressArray,
    poll_status: PollStatus,
    // slot_context is indexed by slot_id (1-255)
    slot_context: [SlotContext; 256],
}
impl Xhci {
    fn new(bdf: BusDeviceFunction) -> Result<Self> {
        let pci = Pci::take();
        pci.disable_interrupt(bdf)?;
        pci.enable_bus_master(bdf)?;
        let bar0 = pci.try_bar0_mem64(bdf)?;
        bar0.disable_cache();

        let cap_regs = unsafe { Mmio::from_raw(bar0.addr() as *mut CapabilityRegisters) };
        if cap_regs.as_ref().hccparams1.read() & 1 == 0 {
            return Err(WasabiError::Failed(
                "HCCPARAMS1.AC64 was 0 (No 64-bit addressing capability)",
            ));
        }
        if cap_regs.as_ref().hccparams1.read() & 4 != 0 {
            return Err(WasabiError::Failed(
                "HCCPARAMS1.CSZ was 1 (Context size is 64, not 32)",
            ));
        }
        let mut op_regs = unsafe {
            Mmio::from_raw(bar0.addr().add(cap_regs.as_ref().length.read() as usize)
                as *mut OperationalRegisters)
        };
        unsafe { op_regs.get_unchecked_mut() }.reset_xhc();
        let rt_regs = unsafe {
            Mmio::from_raw(
                bar0.addr().add(cap_regs.as_ref().rtsoff.read() as usize) as *mut RuntimeRegisters
            )
        };
        let doorbell_regs = unsafe {
            Mmio::from_raw(
                bar0.addr().add(cap_regs.as_ref().dboff.read() as usize) as *mut [u32; 256]
            )
        };
        let portsc = regs::PortSc::new(&bar0, cap_regs.as_ref());
        let scratchpad_buffers = Self::alloc_scratch_pad_buffers(
            op_regs.as_ref().page_size()?,
            cap_regs.as_ref().num_scratch_pad_bufs(),
        )?;
        let device_context_base_array = DeviceContextBaseAddressArray::new(scratchpad_buffers);
        let mut xhc = Xhci {
            bdf,
            cap_regs,
            op_regs,
            rt_regs,
            portsc,
            doorbell_regs,
            command_ring: CommandRing::default(),
            primary_event_ring: EventRing::new()?,
            device_context_base_array,
            poll_status: PollStatus::WaitingSomething,
            slot_context: [(); 256].map(|_| SlotContext::default()),
        };
        xhc.init_primary_event_ring()?;
        xhc.init_slots_and_contexts()?;
        xhc.init_command_ring()?;
        unsafe { xhc.op_regs.get_unchecked_mut() }.start_xhc();

        Ok(xhc)
    }
    fn init_primary_event_ring(&mut self) -> Result<()> {
        let eq = &mut self.primary_event_ring;
        let irs = &mut unsafe { self.rt_regs.get_unchecked_mut() }.irs[0];
        irs.erst_size = 1;
        irs.erdp = eq.ring_phys_addr();
        irs.erst_base = eq.erst_phys_addr();
        irs.management = 0;
        eq.set_erdp(&mut irs.erdp as *mut u64);
        Ok(())
    }
    fn init_slots_and_contexts(&mut self) -> Result<()> {
        let num_slots = self.cap_regs.as_ref().num_of_device_slots();
        unsafe { self.op_regs.get_unchecked_mut() }.set_num_device_slots(num_slots)?;
        unsafe { self.op_regs.get_unchecked_mut() }
            .set_dcbaa_ptr(&mut self.device_context_base_array)?;
        Ok(())
    }
    fn init_command_ring(&mut self) -> Result<()> {
        unsafe {self.op_regs.get_unchecked_mut()}.cmd_ring_ctrl = self.command_ring.ring_phys_addr() | 1 /* Ring Cycle State */;
        Ok(())
    }
    fn alloc_scratch_pad_buffers(
        page_size: usize,
        num_scratch_pad_bufs: usize,
    ) -> Result<Pin<Box<[*mut u8]>>> {
        // 4.20 Scratchpad Buffers
        // This should be done before xHC starts.
        // device_contexts.context[0] points Scratchpad Buffer Arrary.
        // The array contains pointers to a memory region which is sized PAGESIZE and aligned on
        // PAGESIZE. (PAGESIZE can be retrieved from op_regs.PAGESIZE)
        let scratchpad_buffers = ALLOCATOR.alloc_with_options(
            Layout::from_size_align(size_of::<usize>() * num_scratch_pad_bufs, page_size)
                .map_err(error_stringify)?,
        );
        let scratchpad_buffers = unsafe {
            slice::from_raw_parts(scratchpad_buffers as *mut *mut u8, num_scratch_pad_bufs)
        };
        let mut scratchpad_buffers = Pin::new(Box::<[*mut u8]>::from(scratchpad_buffers));
        for sb in scratchpad_buffers.iter_mut() {
            *sb = ALLOCATOR.alloc_with_options(
                Layout::from_size_align(page_size, page_size).map_err(error_stringify)?,
            );
        }
        Ok(scratchpad_buffers)
    }
    fn notify_xhc(&mut self) {
        unsafe {
            write_volatile(&mut self.doorbell_regs.get_unchecked_mut()[0], 0);
        }
    }
    fn notify_ep(&mut self, slot: u8, dci: usize) {
        println!("!!!!!!!! notify slot {} dci {:#10X}", slot, dci);
        unsafe {
            write_volatile(
                &mut self.doorbell_regs.get_unchecked_mut()[slot as usize],
                dci as u32,
            );
        }
    }
    async fn send_command(&mut self, cmd: GenericTrbEntry) -> Result<GenericTrbEntry> {
        let cmd_ptr = self.command_ring.push(cmd)?;
        self.notify_xhc();
        CommandCompletionEventFuture::new(&mut self.primary_event_ring, cmd_ptr)
            .await?
            .ok_or(WasabiError::Failed("Timed out"))
    }
    async fn request_device_descriptor(&mut self, slot: u8) -> Result<GenericTrbEntry> {
        let setup_stage = SetupStageTrb::new(
            SetupStageTrb::REQ_TYPE_DIR_DEVICE_TO_HOST,
            SetupStageTrb::REQ_GET_DESCRIPTOR,
            (DescriptorType::Device as u16) << 8,
            0,
            size_of::<DeviceDescriptor>() as u16,
        );
        let data_stage = DataStageTrb::new_in(
            self.slot_context[slot as usize]
                .device_descriptor()
                .as_mut_slice(),
        );
        let status_stage = StatusStageTrb::new_out();
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring;
        ctrl_ep_ring.push(setup_stage.into())?;
        let trb_ptr_waiting = ctrl_ep_ring.push(data_stage.into())?;
        ctrl_ep_ring.push(status_stage.into())?;
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(WasabiError::Failed("Timed out"))
    }
    async fn request_set_config(&mut self, slot: u8, config_value: u8) -> Result<()> {
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring;
        ctrl_ep_ring.push(
            SetupStageTrb::new(
                0,
                SetupStageTrb::REQ_SET_CONFIGURATION,
                config_value as u16,
                0,
                0,
            )
            .into(),
        )?;
        let trb_ptr_waiting = ctrl_ep_ring.push(StatusStageTrb::new_in().into())?;
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(WasabiError::Failed("Timed out"))?
            .completed()
    }
    async fn request_set_interface(
        &mut self,
        slot: u8,
        interface_number: u8,
        alt_setting: u8,
    ) -> Result<()> {
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring;
        ctrl_ep_ring.push(
            SetupStageTrb::new(
                SetupStageTrb::REQ_TYPE_TO_INTERFACE,
                SetupStageTrb::REQ_SET_INTERFACE,
                alt_setting as u16,
                interface_number as u16,
                0,
            )
            .into(),
        )?;
        let trb_ptr_waiting = ctrl_ep_ring.push(StatusStageTrb::new_in().into())?;
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(WasabiError::Failed("Timed out"))?
            .completed()
    }
    async fn request_set_protocol(
        &mut self,
        slot: u8,
        interface_number: u8,
        protocol: u8,
    ) -> Result<()> {
        // protocol:
        // 0: Boot Protocol
        // 1: Report Protocol
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring;
        ctrl_ep_ring.push(
            SetupStageTrb::new(
                SetupStageTrb::REQ_TYPE_TO_INTERFACE,
                SetupStageTrb::REQ_SET_PROTOCOL,
                protocol as u16,
                interface_number as u16,
                0,
            )
            .into(),
        )?;
        let trb_ptr_waiting = ctrl_ep_ring.push(StatusStageTrb::new_in().into())?;
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(WasabiError::Failed("Timed out"))?
            .completed()
    }
    pub async fn request_report_bytes(&mut self, slot: u8, buf: Pin<&mut [u8]>) -> Result<()> {
        // [HID] 7.2.1 Get_Report Request
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring;
        ctrl_ep_ring.push(
            SetupStageTrb::new(
                SetupStageTrb::REQ_TYPE_DIR_DEVICE_TO_HOST
                    | SetupStageTrb::REQ_TYPE_TYPE_CLASS
                    | SetupStageTrb::REQ_TYPE_TO_INTERFACE,
                SetupStageTrb::REQ_GET_REPORT,
                0x0200, /* Report Type | Report ID */
                0,
                buf.len() as u16,
            )
            .into(),
        )?;
        let trb_ptr_waiting = ctrl_ep_ring.push(DataStageTrb::new_in(buf).into())?;
        ctrl_ep_ring.push(StatusStageTrb::new_out().into())?;
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(WasabiError::Failed("Timed out"))?
            .completed()
    }
    async fn request_descriptor(
        &mut self,
        slot: u8,
        desc_type: DescriptorType,
        desc_index: u8,
        buf: Pin<&mut [u8]>,
    ) -> Result<()> {
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring;
        ctrl_ep_ring.push(
            SetupStageTrb::new(
                SetupStageTrb::REQ_TYPE_DIR_DEVICE_TO_HOST,
                SetupStageTrb::REQ_GET_DESCRIPTOR,
                (desc_type as u16) << 8 | (desc_index as u16),
                0,
                buf.len() as u16,
            )
            .into(),
        )?;
        let trb_ptr_waiting = ctrl_ep_ring.push(DataStageTrb::new_in(buf).into())?;
        ctrl_ep_ring.push(StatusStageTrb::new_out().into())?;
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(WasabiError::Failed("Timed out"))?
            .completed()
    }
    async fn request_config_descriptor_and_rest(&mut self, slot: u8) -> Result<Vec<UsbDescriptor>> {
        let mut config_descriptor = Box::pin(ConfigDescriptor::default());
        self.request_descriptor(
            slot,
            DescriptorType::Config,
            0,
            config_descriptor.as_mut().as_mut_slice(),
        )
        .await?;
        let mut buf = Vec::<u8>::new();
        buf.resize(config_descriptor.total_length(), 0);
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        self.request_descriptor(slot, DescriptorType::Config, 0, buf.as_mut())
            .await?;
        let iter = DescriptorIterator::new(&buf);
        let descriptors: Vec<UsbDescriptor> = iter.collect();
        Ok(descriptors)
    }
    async fn request_string_descriptor_zero(&mut self, slot: u8) -> Result<Pin<Box<[u8]>>> {
        let mut buf = Vec::<u8>::new();
        buf.resize(128, 0);
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        self.request_descriptor(slot, DescriptorType::String, 0, buf.as_mut())
            .await?;
        Ok(buf)
    }
    async fn request_string_descriptor(&mut self, slot: u8, index: u8) -> Result<String> {
        let mut buf = Vec::<u8>::new();
        buf.resize(128, 0);
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        self.request_descriptor(slot, DescriptorType::String, index, buf.as_mut())
            .await?;
        Ok(String::from_utf8_lossy(&buf[2..]).to_string())
    }
    async fn ensure_ring_is_working(&mut self) -> Result<()> {
        for i in 0..100 {
            println!("nop #{}", i);
            self.send_command(GenericTrbEntry::cmd_no_op())
                .await?
                .completed()?;
        }
        Ok(())
    }
    async fn attach_device(&mut self, port: usize) -> Result<()> {
        let e = self
            .send_command(GenericTrbEntry::cmd_enable_slot())
            .await?;
        let slot = e.slot_id();
        println!(
            "USB3 Device Detected, slot = {}, port {}",
            e.slot_id(),
            port
        );

        // Setup an input context and send AddressDevice command.
        // 4.3.3 Device Slot Initialization
        let slot_context = &mut self.slot_context[slot as usize];
        self.device_context_base_array
            .set_output_context(slot, slot_context.output_context());
        let mut input_ctrl_ctx = InputControlContext::default();
        input_ctrl_ctx.add_context(0)?;
        input_ctrl_ctx.add_context(1)?;
        let input_context = &mut slot_context.input_context.as_mut();
        input_context.set_input_ctrl_ctx(input_ctrl_ctx)?;
        // 3. Initialize the Input Slot Context data structure (6.2.2)
        input_context.set_root_hub_port_number(port)?;
        input_context.set_last_valid_dci(1)?;
        // 4. Initialize the Transfer Ring for the Default Control Endpoint
        // 5. Initialize the Input default control Endpoint 0 Context (6.2.3)
        let portsc = self.portsc.get(port)?;
        input_context.set_port_speed(portsc.port_speed())?;
        let ctrl_ep_ring_phys_addr = slot_context.ctrl_ep_ring.ring_phys_addr();
        input_context.set_ep_ctx(
            1,
            EndpointContext::new_control_endpoint(
                portsc.max_packet_size()?,
                ctrl_ep_ring_phys_addr,
            )?,
        )?;
        // 8. Issue an Address Device Command for the Device Slot
        let cmd = GenericTrbEntry::cmd_address_device(input_context.as_ref(), slot);
        self.send_command(cmd).await?.completed()?;
        self.request_device_descriptor(slot).await?.completed()?;
        let descriptors = self.request_config_descriptor_and_rest(slot).await?;
        let device_descriptor = *self.slot_context[slot as usize].device_descriptor();
        println!("{:?}", device_descriptor);
        let lang_id_table = self.request_string_descriptor_zero(slot).await?;
        println!("{:?}", lang_id_table);
        let vendor_name = self
            .request_string_descriptor(slot, device_descriptor.manufacturer_idx)
            .await?;
        let product_name = self
            .request_string_descriptor(slot, device_descriptor.product_idx)
            .await?;
        println!("{} {}", vendor_name, product_name);
        let mut last_config: Option<ConfigDescriptor> = None;
        let mut boot_keyboard_interface: Option<InterfaceDescriptor> = None;
        let mut ep_desc_list: Vec<EndpointDescriptor> = Vec::new();
        if device_descriptor.device_class == 0 {
            // Device class is derived from Interface Descriptor
            for d in &descriptors {
                println!("{:?}", d);
            }
            for d in &descriptors {
                match d {
                    UsbDescriptor::Config(e) => {
                        if boot_keyboard_interface.is_some() {
                            break;
                        }
                        last_config = Some(*e);
                        ep_desc_list.clear();
                    }
                    UsbDescriptor::Interface(e) => {
                        if let (3, 1, 1) = e.triple() {
                            boot_keyboard_interface = Some(*e)
                        }
                    }
                    UsbDescriptor::Endpoint(e) => {
                        ep_desc_list.push(*e);
                    }
                    _ => {}
                }
            }
            if let Some(interface_desc) = boot_keyboard_interface {
                println!("!!!!! USB KBD Found!");
                println!("Configuring Endpoints");
                let slot_context = &mut self.slot_context[slot as usize];
                let input_context = &mut slot_context.input_context.as_mut();
                let mut input_ctrl_ctx = InputControlContext::default();
                input_ctrl_ctx.add_context(0)?;
                const EP_RING_NONE: Option<TransferRing> = None;
                let mut ep_rings = [EP_RING_NONE; 32];
                let mut last_dci = 1;
                for ep_desc in ep_desc_list {
                    match EndpointType::from(&ep_desc) {
                        EndpointType::InterruptIn => {
                            println!("Initializing {:?}", ep_desc);
                            let tring = TransferRing::new()?;
                            input_ctrl_ctx.add_context(ep_desc.dci())?;
                            input_context.set_ep_ctx(
                                ep_desc.dci(),
                                EndpointContext::new_interrupt_in_endpoint(
                                    portsc.max_packet_size()?,
                                    tring.ring_phys_addr(),
                                    portsc.port_speed(),
                                    ep_desc.interval,
                                    8,
                                )?,
                            )?;
                            last_dci = max(last_dci, ep_desc.dci());
                            ep_rings[ep_desc.dci()] = Some(tring);
                        }
                        _ => {
                            println!("Ignoring {:?}", ep_desc);
                        }
                    }
                }
                input_context.set_last_valid_dci(last_dci)?;
                input_context.set_input_ctrl_ctx(input_ctrl_ctx)?;
                let cmd = GenericTrbEntry::cmd_configure_endpoint(input_context.as_ref(), slot);
                self.send_command(cmd).await?.completed()?;
                println!("setting config");
                self.request_set_config(slot, last_config.expect("no config").config_value())
                    .await?;
                println!("setting interface");
                self.request_set_interface(
                    slot,
                    interface_desc.interface_number(),
                    interface_desc.alt_setting(),
                )
                .await?;
                println!("setting protocol");
                self.request_set_protocol(
                    slot,
                    interface_desc.interface_number(),
                    0, /* Boot Protocol */
                )
                .await?;
                println!("notify_ep");
                // 4.6.6 Configure Endpoint
                // When configuring or deconfiguring a device, only after completing a successful
                // Configure Endpoint Command and a successful USB SET_CONFIGURATION
                // request may software schedule data transfers through a newly enabled endpoint
                // or Stream Transfer Ring of the Device Slot.
                for (dci, tring) in ep_rings.iter_mut().enumerate() {
                    match tring {
                        Some(tring) => {
                            tring.fill_ring()?;
                            self.notify_ep(slot, dci);
                        }
                        None => {}
                    }
                }
                println!("USB KBD init done!");
                loop {
                    let event_trb =
                        TransferEventFuture::new_on_slot(&mut self.primary_event_ring, slot).await;
                    match event_trb {
                        Ok(Some(trb)) => {
                            let transfer_trb_ptr = trb.data.read() as usize;
                            let report = unsafe {
                                Mmio::<[u8; 8]>::from_raw(
                                    *(transfer_trb_ptr as *const usize) as *mut [u8; 8],
                                )
                            };
                            println!("recv: {:?}", report.as_ref());
                            if let Some(ref mut tring) = ep_rings[trb.dci()] {
                                tring.dequeue_trb(transfer_trb_ptr)?;
                                self.notify_ep(slot, trb.dci());
                            }
                        }
                        Ok(None) => {
                            // Timed out. Do nothing.
                        }
                        Err(e) => {
                            println!("e: {:?}", e);
                        }
                    }
                }
            }
        }
        Ok(())
    }
    async fn poll(&mut self) -> Result<()> {
        match self.poll_status {
            PollStatus::WaitingSomething => {
                for regs::PortScIteratorItem { port, portsc } in self.portsc.iter() {
                    let state = portsc.state();
                    if let regs::PortState::Disabled = state {
                        // Reset port to Enable the port (via Reset state)
                        println!(
                            "Resetting port: prev state: portsc[port = {}] = {:#10X} {:?} {:?}",
                            port,
                            portsc.value(),
                            portsc.state(),
                            portsc
                        );
                        portsc.reset();
                        self.poll_status = PollStatus::EnablingPort { port };
                        break;
                    }
                }
            }
            PollStatus::EnablingPort { port } => {
                println!("Enabling Port {}...", port);
                let portsc = self.portsc.get(port)?;
                if let PortState::Enabled = portsc.state() {
                    if let PortLinkState::U0 = portsc.pls() {
                        // Attached USB3 device
                        self.poll_status = PollStatus::USB3Attached { port };
                    }
                }
            }
            PollStatus::USB3Attached { port } => {
                if let Err(e) = self.attach_device(port).await {
                    println!(
                        "Failed to initialize an USB device on port {}: {:?}",
                        port, e
                    );
                }
                self.poll_status = PollStatus::WaitingSomething;
            }
        }
        Ok(())
    }
}
struct XhciDriverInstance {}
impl XhciDriverInstance {
    fn new(bdf: BusDeviceFunction, executor: Rc<SyncUnsafeCell<Executor>>) -> Result<Self> {
        unsafe { &mut *executor.get() }.spawn(Task::new(async move {
            let mut xhc = Xhci::new(bdf)?;
            xhc.ensure_ring_is_working().await?;
            loop {
                if let Err(e) = xhc.poll().await {
                    break Err(e);
                } else {
                    yield_execution().await;
                }
            }
        }));
        Ok(Self {})
    }
}
impl PciDeviceDriverInstance for XhciDriverInstance {
    fn name(&self) -> &str {
        "XhciDriverInstance"
    }
}
