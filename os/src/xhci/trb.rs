extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::util::extract_bits;
use crate::volatile::Volatile;
use crate::xhci::context::InputContext;
use crate::xhci::TrbRing;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::format;
use core::mem::size_of;
use core::mem::transmute;
use core::pin::Pin;

#[derive(Debug, Copy, Clone)]
#[repr(u32)]
#[non_exhaustive]
#[derive(PartialEq, Eq)]
pub enum TrbType {
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

// 6.4.5 TRB Completion Code
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
#[non_exhaustive]
#[derive(PartialEq, Eq)]
pub enum CompletionCode {
    Success = 1,
    BabbleDetectedError = 3,
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
pub struct GenericTrbEntry {
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
    pub fn data(&self) -> u64 {
        self.data.read()
    }
    pub fn cycle_state(&self) -> bool {
        self.control.read_bits(0, 1) != 0
    }
    pub fn set_cycle_state(&mut self, cycle: bool) {
        self.control.write_bits(0, 1, cycle.into()).unwrap()
    }
    pub fn set_toggle_cycle(&mut self, value: bool) {
        self.control.write_bits(1, 1, value.into()).unwrap()
    }
    pub fn trb_type(&self) -> u32 {
        self.control.read_bits(10, 6)
    }
    pub fn set_trb_type(&mut self, trb_type: TrbType) {
        self.control.write_bits(10, 6, trb_type as u32).unwrap()
    }
    pub fn dci(&self) -> usize {
        extract_bits(self.control.read(), 16, 5)
            .try_into()
            .expect("Invalid ep dci")
    }
    pub fn completion_code(&self) -> u32 {
        self.option.read_bits(24, 8)
    }
    pub fn transfer_length(&self) -> usize {
        self.option.read_bits(0, 24) as usize
    }
    pub fn slot_id(&self) -> u8 {
        self.control.read_bits(24, 8).try_into().unwrap()
    }
    pub fn set_slot_id(&mut self, slot: u8) {
        self.control.write_bits(24, 8, slot as u32).unwrap()
    }
    pub fn completed(&self) -> Result<()> {
        if self.trb_type() != TrbType::CommandCompletionEvent as u32
            && self.trb_type() != TrbType::TransferEvent as u32
        {
            Err(Error::FailedString(format!(
                "Expected TrbType == CommandCompletionEvent or TransferEvent but got {}",
                self.trb_type(),
            )))
        } else if self.completion_code() != CompletionCode::Success as u32
            && self.completion_code() != CompletionCode::ShortPacket as u32
        {
            Err(Error::FailedString(format!(
                "Expected CompletionCode == Success but got {} ({})",
                self.completion_code() as u32,
                CompletionCode::parse(self.completion_code())
            )))
        } else {
            Ok(())
        }
    }
    pub fn cmd_no_op() -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::NoOpCommand);
        trb
    }
    pub fn cmd_enable_slot() -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::EnableSlotCommand);
        trb
    }
    pub fn cmd_address_device(input_context: Pin<&InputContext>, slot_id: u8) -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::AddressDeviceCommand);
        trb.data
            .write(input_context.get_ref() as *const InputContext as u64);
        trb.set_slot_id(slot_id);
        trb
    }
    pub fn cmd_evaluate_context(input_context: Pin<&InputContext>, slot_id: u8) -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::EvaluateContextCommand);
        trb.data
            .write(input_context.get_ref() as *const InputContext as u64);
        trb.set_slot_id(slot_id);
        trb
    }
    pub fn cmd_configure_endpoint(input_context: Pin<&InputContext>, slot_id: u8) -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::ConfigureEndpointCommand);
        trb.data
            .write(input_context.get_ref() as *const InputContext as u64);
        trb.set_slot_id(slot_id);
        trb
    }
    pub fn trb_link(ring: &TrbRing) -> Self {
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
pub struct NormalTrb {
    buf: u64,
    option: u32,
    control: u32,
}
const _: () = assert!(size_of::<DataStageTrb>() == 16);
impl NormalTrb {
    const CONTROL_INTERRUPT_ON_COMPLETION: u32 = 1 << 5;
    const CONTROL_INTERRUPT_ON_SHORT_PACKET: u32 = 1 << 2;
    pub fn new(buf: *mut u8, size: u16) -> Self {
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
pub struct SetupStageTrb {
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
    pub const REQ_TYPE_DIR_DEVICE_TO_HOST: u8 = 1 << 7;
    //pub const REQ_TYPE_DIR_HOST_TO_DEVICE: u8 = 0 << 7;
    // bmRequest bit[5..=6]: Request Type
    //      0: Standard
    //      1: Class
    //      2: Vendor
    //      _: Reserved
    //pub const REQ_TYPE_TYPE_STANDARD: u8 = 0 << 5;
    pub const REQ_TYPE_TYPE_CLASS: u8 = 1 << 5;
    //pub const REQ_TYPE_TYPE_VENDOR: u8 = 2 << 5;
    // bmRequest bit[0..=4]: Recipient
    //      0: Device
    //      1: Interface
    //      2: Endpoint
    //      3: Other
    //      _: Reserved
    //pub const REQ_TYPE_TO_DEVICE: u8 = 0;
    pub const REQ_TYPE_TO_INTERFACE: u8 = 1;
    //pub const REQ_TYPE_TO_ENDPOINT: u8 = 2;
    //pub const REQ_TYPE_TO_OTHER: u8 = 3;

    pub const REQ_GET_REPORT: u8 = 1;
    pub const REQ_GET_DESCRIPTOR: u8 = 6;
    pub const REQ_SET_CONFIGURATION: u8 = 9;
    pub const REQ_SET_INTERFACE: u8 = 11;
    pub const REQ_SET_PROTOCOL: u8 = 0x0b;
    pub fn new(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Self {
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
pub struct DataStageTrb {
    buf: u64,
    option: u32,
    control: u32,
}
const _: () = assert!(size_of::<DataStageTrb>() == 16);
impl DataStageTrb {
    pub fn new_in<T: Sized>(buf: Pin<&mut [T]>) -> Self {
        Self {
            buf: buf.as_ptr() as u64,
            option: (buf.len() * size_of::<T>()) as u32,
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
pub struct StatusStageTrb {
    reserved: u64,
    option: u32,
    control: u32,
}
const _: () = assert!(size_of::<StatusStageTrb>() == 16);
impl StatusStageTrb {
    pub fn new_out() -> Self {
        Self {
            reserved: 0,
            option: 0,
            control: (TrbType::StatusStage as u32) << 10,
        }
    }
    pub fn new_in() -> Self {
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
