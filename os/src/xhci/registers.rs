extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::mutex::Mutex;
use crate::pci::BarMem64;
use crate::print;
use crate::println;
use crate::util::extract_bits;
use crate::util::PAGE_SIZE;
use crate::volatile::Volatile;
use crate::x86_64::busy_loop_hint;
use crate::xhci::ring::CommandRing;
use crate::xhci::ring::EventRing;
use crate::xhci::DeviceContextBaseAddressArray;
use crate::xhci::RawDeviceContextBaseAddressArray;
use alloc::fmt;
use alloc::fmt::Debug;
use alloc::format;
use alloc::rc::Rc;
use alloc::rc::Weak;
use alloc::vec::Vec;
use core::mem::size_of;
use core::mem::transmute;
use core::ptr::read_volatile;
use core::ptr::write_volatile;

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum UsbMode {
    Unknown(u32),
    FullSpeed,
    LowSpeed,
    HighSpeed,
    SuperSpeed,
}
impl UsbMode {
    pub fn psi(&self) -> u32 {
        match *self {
            Self::FullSpeed => 1,
            Self::LowSpeed => 2,
            Self::HighSpeed => 3,
            Self::SuperSpeed => 4,
            Self::Unknown(psi) => psi,
        }
    }
}

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

#[derive(Debug, Eq, PartialEq)]
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

#[repr(C)]
pub struct PortScWrapper {
    ptr: Mutex<*mut u32>,
}
impl PortScWrapper {
    const PRESERVE_MASK: u32 = 0b01001111000000011111111111101001;
    const BIT_CURRENT_CONNECT_STATUS: u32 = 1 << 0;
    const BIT_PORT_ENABLED_DISABLED: u32 = 1 << 1;
    const BIT_PORT_RESET: u32 = 1 << 4;
    const BIT_PORT_POWER: u32 = 1 << 9;
    const BIT_CONNECT_STATUS_CHANGE: u32 = 1 << 17;
    const BIT_PORT_RESET_CHANGE: u32 = 1 << 21;
    fn new(ptr: *mut u32) -> Self {
        Self {
            ptr: Mutex::new(ptr, "portsc"),
        }
    }
    pub fn value(&self) -> u32 {
        let portsc = self.ptr.lock();
        unsafe { read_volatile(*portsc) }
    }
    pub fn set_bits(&self, bits: u32) {
        let portsc = self.ptr.lock();
        let old = unsafe { read_volatile(*portsc) };
        unsafe { write_volatile(*portsc, (old & Self::PRESERVE_MASK) | bits) }
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
    pub fn csc(&self) -> bool {
        // CSC - Connect Status Change - RW1CS
        self.value() & Self::BIT_CONNECT_STATUS_CHANGE != 0
    }
    pub fn clear_csc(&self) {
        self.set_bits(Self::BIT_CONNECT_STATUS_CHANGE);
    }
    pub fn prc(&self) -> bool {
        // PRC - Port Reset Change - RW1CS
        self.value() & Self::BIT_PORT_RESET_CHANGE != 0
    }
    pub fn clear_prc(&self) {
        self.set_bits(Self::BIT_PORT_RESET_CHANGE);
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
            speed => Err(Error::FailedString(format!(
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
            "PORTSC: {:#010X} {:?} (PP={:?}, PLS={:?}, Speed={:?})",
            self.value(),
            self.state(),
            self.pp(),
            self.pls(),
            self.port_speed(),
        )
    }
}
// Iterator over PortSc
pub struct PortScIteratorItem {
    pub port: usize,
    pub portsc: Weak<PortScWrapper>,
}
pub struct PortScIterator<'a> {
    list: &'a PortSc,
    next_port: usize,
    next_port_back: usize,
}
impl<'a> Iterator for PortScIterator<'a> {
    type Item = PortScIteratorItem;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next_port <= self.next_port_back {
            let port = self.next_port;
            let portsc = self.list.get(port).ok()?;
            self.next_port += 1;
            Some(PortScIteratorItem { port, portsc })
        } else {
            None
        }
    }
}
impl<'a> DoubleEndedIterator for PortScIterator<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.next_port <= self.next_port_back {
            let port = self.next_port_back;
            let portsc = self.list.get(port).ok()?;
            self.next_port_back -= 1;
            Some(PortScIteratorItem { port, portsc })
        } else {
            None
        }
    }
}
// Interface to access PORTSC registers
//
// [xhci] 5.4.8: PORTSC
// OperationalBase + (0x400 + 0x10 * (n - 1))
// where n = Port Number (1, 2, ..., MaxPorts)
pub struct PortSc {
    entries: Vec<Rc<PortScWrapper>>,
}
impl PortSc {
    pub fn new(bar: &BarMem64, cap_regs: &CapabilityRegisters) -> Self {
        let base = unsafe { bar.addr().add(cap_regs.length()).add(0x400) } as *mut u32;
        let num_ports = cap_regs.num_of_ports();
        println!("PORTSC @ {:p}, max_port_num = {}", base, num_ports);
        let mut entries = Vec::new();
        for port in 1..=num_ports {
            // SAFETY: This is safe since the result of ptr calculation
            // always points to a valid PORTSC entry under the condition.
            let ptr = unsafe { base.add((port - 1) * 4) };
            entries.push(Rc::new(PortScWrapper::new(ptr)));
        }
        assert!(entries.len() == num_ports);
        Self { entries }
    }
    pub fn get(&self, port: usize) -> Result<Weak<PortScWrapper>> {
        self.entries
            .get(port.wrapping_sub(1))
            .ok_or("xHC: Port Number Out of Range".into())
            .map(Rc::downgrade)
    }
    pub fn iter(&self) -> PortScIterator {
        PortScIterator {
            list: self,
            // Note: valid ports are 1..=self.num_ports
            next_port: 1,
            next_port_back: self.entries.len(),
        }
    }
}

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
    pub fn length(&self) -> usize {
        self.length.read() as usize
    }
    pub fn rtsoff(&self) -> usize {
        self.rtsoff.read() as usize
    }
    pub fn dboff(&self) -> usize {
        self.dboff.read() as usize
    }
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
    pub fn assert_capabilities(&self) -> Result<()> {
        if self.hccparams1.read() & 1 == 0 {
            return Err(Error::Failed(
                "HCCPARAMS1.AC64 was 0 (No 64-bit addressing capability)",
            ));
        }
        if self.hccparams1.read() & 4 != 0 {
            return Err(Error::Failed(
                "HCCPARAMS1.CSZ was 1 (Context size is 64, not 32)",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct OperationalRegisters {
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
    pub fn page_size(&self) -> Result<usize> {
        let page_size_bits = unsafe { read_volatile(&self.page_size) } & 0xFFFF;
        // bit[n] of page_size_bits is set => PAGE_SIZE will be 2^(n+12).
        if page_size_bits.count_ones() != 1 {
            return Err(Error::Failed("PAGE_SIZE has multiple bits set"));
        }
        let page_size_shift = page_size_bits.trailing_zeros();
        Ok(1 << (page_size_shift + 12))
    }
    pub fn set_num_device_slots(&mut self, num: usize) -> Result<()> {
        unsafe {
            let c = read_volatile(&self.config);
            let c = c & !0xFF;
            let c = c | u64::try_from(num)?;
            write_volatile(&mut self.config, c);
        }
        Ok(())
    }
    pub fn set_dcbaa_ptr(&mut self, dcbaa: &mut DeviceContextBaseAddressArray) -> Result<()> {
        unsafe {
            write_volatile(
                &mut self.device_ctx_base_addr_array_ptr,
                dcbaa.inner_mut_ptr(),
            );
        }
        Ok(())
    }
    pub fn set_cmd_ring_ctrl(&mut self, ring: &CommandRing) {
        self.cmd_ring_ctrl = ring.ring_phys_addr() | 1 /* Consumer Ring Cycle State */
    }
    pub fn assert_params(&self) -> Result<()> {
        assert_eq!(self.page_size()?, PAGE_SIZE);
        Ok(())
    }
    pub fn reset_xhc(&mut self) {
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
    }
    pub fn start_xhc(&mut self) {
        self.set_command_bits(Self::CMD_RUN_STOP);
        while self.status() & Self::STATUS_HC_HALTED != 0 {
            print!(".");
            busy_loop_hint();
        }
        println!("xHC started");
    }
}

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

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct RuntimeRegisters {
    rsvdz: [u32; 8],
    irs: [InterrupterRegisterSet; 1024],
}
impl RuntimeRegisters {
    pub fn init_irs(&mut self, index: usize, ring: &mut EventRing) -> Result<()> {
        let irs = self
            .irs
            .get_mut(index)
            .ok_or(Error::Failed("Index out of range"))?;
        irs.erst_size = 1;
        irs.erdp = ring.ring_phys_addr();
        irs.erst_base = ring.erst_phys_addr();
        irs.management = 0;
        ring.set_erdp(&mut irs.erdp as *mut u64);
        Ok(())
    }
}
const _: () = assert!(size_of::<RuntimeRegisters>() == 0x8020);
