extern crate alloc;

use crate::info;
use crate::mmio::IoBox;
use crate::print::hexdump_struct;
use crate::result::Result;
use crate::slice::Sliceable;
use crate::xhci::Controller;
use crate::xhci::EndpointContext;
use crate::xhci::GenericTrbEntry;
use crate::xhci::InputContext;
use crate::xhci::InputControlContext;
use crate::xhci::TransferRing;
use crate::xhci::UsbMode;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomPinned;
use core::mem::size_of;
use core::time::Duration;

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
#[non_exhaustive]
#[allow(unused)]
#[derive(PartialEq, Eq)]
pub enum UsbDescriptorType {
    Device = 1,
    Config = 2,
    String = 3,
    Interface = 4,
    Endpoint = 5,
    Hid = 0x21,
    Report = 0x22,
}

#[derive(Debug, Clone)]
pub enum UsbDescriptor {
    Config(ConfigDescriptor),
    Endpoint(EndpointDescriptor),
    Interface(InterfaceDescriptor),
    Hid(HidDescriptor),
    Unknown {
        desc_len: u8,
        desc_type: u8,
        payload: Vec<u8>,
    },
}

#[derive(Debug, Copy, Clone, Default)]
#[allow(unused)]
#[repr(packed)]
pub struct UsbDeviceDescriptor {
    pub desc_length: u8,
    pub desc_type: u8,
    pub version: u16,
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub max_packet_size: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_version: u16,
    pub manufacturer_idx: u8,
    pub product_idx: u8,
    pub serial_idx: u8,
    pub num_of_config: u8,
}
const _: () = assert!(size_of::<UsbDeviceDescriptor>() == 18);
unsafe impl Sliceable for UsbDeviceDescriptor {}

#[derive(Debug, Copy, Clone, Default)]
#[allow(unused)]
#[repr(packed)]
pub struct ConfigDescriptor {
    desc_length: u8,
    desc_type: u8,
    total_length: u16,
    num_of_interfaces: u8,
    config_value: u8,
    config_string_index: u8,
    attribute: u8,
    max_power: u8,
    //
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<ConfigDescriptor>() == 9);
impl ConfigDescriptor {
    pub fn total_length(&self) -> usize {
        self.total_length as usize
    }
    pub fn config_value(&self) -> u8 {
        self.config_value
    }
}
unsafe impl Sliceable for ConfigDescriptor {}

pub struct DescriptorIterator<'a> {
    buf: &'a [u8],
    index: usize,
}
impl<'a> DescriptorIterator<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, index: 0 }
    }
}
impl<'a> Iterator for DescriptorIterator<'a> {
    type Item = UsbDescriptor;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.buf.len() {
            None
        } else {
            let buf = &self.buf[self.index..];
            let desc_len = buf[0];
            let desc_type = buf[1];
            let desc = match desc_type {
                e if e == UsbDescriptorType::Config as u8 => {
                    UsbDescriptor::Config(
                        ConfigDescriptor::copy_from_slice(buf).ok()?,
                    )
                }
                e if e == UsbDescriptorType::Interface as u8 => {
                    UsbDescriptor::Interface(
                        InterfaceDescriptor::copy_from_slice(buf).ok()?,
                    )
                }
                e if e == UsbDescriptorType::Endpoint as u8 => {
                    UsbDescriptor::Endpoint(
                        EndpointDescriptor::copy_from_slice(buf).ok()?,
                    )
                }
                e if e == UsbDescriptorType::Hid as u8 => UsbDescriptor::Hid(
                    HidDescriptor::copy_from_slice(buf).ok()?,
                ),
                _ => UsbDescriptor::Unknown {
                    desc_len,
                    desc_type,
                    payload: buf[2..(desc_len as usize)].to_vec(),
                },
            };
            self.index += desc_len as usize;
            Some(desc)
        }
    }
}

#[derive(Copy, Clone, Default)]
#[allow(unused)]
#[repr(packed)]
pub struct InterfaceDescriptor {
    desc_length: u8,
    desc_type: u8,
    pub interface_number: u8,
    pub alt_setting: u8,
    num_of_endpoints: u8,
    interface_class: u8,
    interface_subclass: u8,
    interface_protocol: u8,
    interface_index: u8,
}
const _: () = assert!(size_of::<InterfaceDescriptor>() == 9);
unsafe impl Sliceable for InterfaceDescriptor {}
impl fmt::Debug for InterfaceDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "IfaceDesc {{ num={}, alt={}, csp={:#04X}:{:#04X}:{:#04X} }}",
            self.interface_number,
            self.alt_setting,
            self.interface_class,
            self.interface_subclass,
            self.interface_protocol
        )
    }
}
impl InterfaceDescriptor {
    pub fn triple(&self) -> (u8, u8, u8) {
        (
            self.interface_class,
            self.interface_subclass,
            self.interface_protocol,
        )
    }
}

#[derive(Copy, Clone, Default)]
#[allow(unused)]
#[repr(C, packed)]
pub struct EndpointDescriptor {
    pub desc_length: u8,
    pub desc_type: u8,

    // endpoint_address:
    //   - bit[0..=3]: endpoint number
    //   - bit[7]: direction(0: out, 1: in)
    pub endpoint_address: u8,

    // attributes:
    //   - bit[0..=1]: transfer type(0: Control, 1: Isochronous, 2: Bulk, 3:
    //     Interrupt)
    pub attributes: u8,
    pub max_packet_size: u16,
    // interval:
    // [xhci] Table 6-12
    // interval_ms = interval (For FS/LS Interrupt) (1-255) (3-10)
    // interval_ms = 2^(interval-1) (For FS Isoch) (1-16) (3-18)
    // interval_us = 2^(interval-1) * 125us (For SSP/SS/HS) (1-16) (0-15)
    //
    // For example, if bInterval = 11 for SS Interrupt, Interval will be 10.
    pub interval: u8,
}
impl EndpointDescriptor {
    fn ep_num(&self) -> u8 {
        self.endpoint_address & 0b1111
    }
    pub fn is_dir_in(&self) -> bool {
        self.endpoint_address & (1 << 7) != 0
    }
    pub fn dci(&self) -> usize {
        let dci = self.ep_num() * 2 + u8::from(self.is_dir_in());
        dci as usize
    }
    fn ep_type_val(&self) -> u8 {
        self.attributes & 0b11
    }
    pub fn is_interrupt_endpoint(&self) -> bool {
        self.ep_type_val() == 3
    }
    pub fn is_bulk_endpoint(&self) -> bool {
        self.ep_type_val() == 2
    }
    fn ep_type_str(&self) -> &str {
        match self.ep_type_val() {
            0 => "Control",
            1 => "Isochronous",
            2 => "Bulk",
            3 => "Interrupt",
            _ => "Unknown",
        }
    }
    fn ep_dir_str(&self) -> &str {
        if self.endpoint_address & (1 << 7) != 0 {
            "IN"
        } else {
            "OUT"
        }
    }
    pub fn calc_interval_time(&self, port_speed: UsbMode) -> Result<Duration> {
        // [xhci] 6-12: Endpoint Type vs. Interval Calculation
        if self.is_interrupt_endpoint() {
            if matches!(port_speed, UsbMode::HighSpeed | UsbMode::SuperSpeed)
                && self.is_interrupt_endpoint()
            {
                if (1..=16).contains(&self.interval) {
                    return Ok(Duration::from_micros(
                        125 << (self.interval - 1),
                    ));
                } else {
                    return Err("bInterval out of range");
                }
            } else if matches!(
                port_speed,
                UsbMode::FullSpeed | UsbMode::LowSpeed
            ) {
                if (1..=16).contains(&self.interval) {
                    return Ok(Duration::from_millis(self.interval as u64));
                } else {
                    return Err("bInterval out of range");
                }
            }
        }
        Err("Unimplemented combination for Interval calc")
    }
}
impl fmt::Debug for EndpointDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let max_packet_size = self.max_packet_size;
        let interval = self.interval;
        write!(
            f,
            "EPDesc {{ ep {}: {:11} {:3}, mps:{:#06X}, interval:{} }}",
            self.ep_num(),
            self.ep_type_str(),
            self.ep_dir_str(),
            max_packet_size,
            interval
        )
    }
}
const _: () = assert!(size_of::<EndpointDescriptor>() == 7);
unsafe impl Sliceable for EndpointDescriptor {}

// [hid_1_11]:
pub const TRIPLE_FOR_HID_BOOT_KBD: (u8, u8, u8) = (
    3, /* HID Class */
    1, /* Boot Interface Subclass */
    1, /* Keyboard */
);

// [hid_1_11]:
// 7.2.5 Get_Protocol Request
// 7.2.6 Set_Protocol Request
#[repr(u8)]
pub enum UsbHidProtocol {
    BootProtocol = 0,
    ReportProtocol = 1,
}

// [hid_1_11] 7.2.1
#[repr(u8)]
pub enum UsbHidReportType {
    Input = 1,
    Output = 2,
    Feature = 3,
}

pub async fn request_device_descriptor(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
) -> Result<UsbDeviceDescriptor> {
    let buf = vec![0; size_of::<UsbDeviceDescriptor>()];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_descriptor(
        slot,
        ctrl_ep_ring,
        UsbDescriptorType::Device,
        0,
        0,
        &mut buf,
    )
    .await?;
    UsbDeviceDescriptor::copy_from_slice(buf.as_ref().get_ref())
}
pub async fn request_string_descriptor(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
    lang_id: u16,
    index: u8,
) -> Result<String> {
    let buf = vec![0; 128];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_descriptor(
        slot,
        ctrl_ep_ring,
        UsbDescriptorType::String,
        index,
        lang_id,
        &mut buf,
    )
    .await?;
    Ok(String::from_utf8_lossy(&buf[2..])
        .to_string()
        .replace('\0', ""))
}

pub async fn request_string_descriptor_zero(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
) -> Result<Vec<u8>> {
    let buf = vec![0; 8];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_descriptor(
        slot,
        ctrl_ep_ring,
        UsbDescriptorType::String,
        0,
        0,
        &mut buf,
    )
    .await?;
    Ok(buf.as_ref().get_ref().to_vec())
}
pub async fn request_config_descriptor_and_rest(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
    desc_index: u8,
) -> Result<Vec<UsbDescriptor>> {
    let buf = vec![0u8; size_of::<ConfigDescriptor>()];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_descriptor(
        slot,
        ctrl_ep_ring,
        UsbDescriptorType::Config,
        desc_index,
        0,
        &mut buf,
    )
    .await?;
    let config_descriptor =
        ConfigDescriptor::copy_from_slice(buf.as_ref().get_ref())?;
    let buf = vec![0; config_descriptor.total_length()];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_descriptor(
        slot,
        ctrl_ep_ring,
        UsbDescriptorType::Config,
        desc_index,
        0,
        &mut buf,
    )
    .await?;
    let iter = DescriptorIterator::new(&buf);
    let descriptors: Vec<UsbDescriptor> = iter.collect();
    Ok(descriptors)
}
pub async fn request_hid_report(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
) -> Result<Vec<u8>> {
    let buf = vec![0u8; 8];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_report_bytes(
        slot,
        ctrl_ep_ring,
        1,
        &mut buf,
        0,
        UsbHidReportType::Output as u16,
    )
    .await?;
    Ok(buf.to_vec())
}
pub async fn request_hid_input_report(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
    dci: usize,
    interface_number: u8,
) -> Result<Vec<u8>> {
    let buf = vec![0u8; 8];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    let r = xhc
        .request_report_bytes(
            slot,
            ctrl_ep_ring,
            dci,
            &mut buf,
            interface_number,
            UsbHidReportType::Input as u16,
        )
        .await?;
    Ok(buf[0..r].to_vec())
}
pub async fn get_hid_keyboard_input_report(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
    dci: usize,
    interface_number: u8,
) -> Result<Vec<u8>> {
    let buf = vec![0u8; 8];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    let r = xhc
        .request_report_bytes(
            slot,
            ctrl_ep_ring,
            dci,
            &mut buf,
            interface_number,
            UsbHidReportType::Input as u16,
        )
        .await?;
    Ok(buf[0..r].to_vec())
}
pub async fn get_hid_keyboard_output_report(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
    dci: usize,
    interface_number: u8,
) -> Result<u8> {
    let buf = vec![0u8];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_report_bytes(
        slot,
        ctrl_ep_ring,
        dci,
        &mut buf,
        interface_number,
        2,
    )
    .await?;
    Ok(buf[0])
}
pub async fn set_hid_keyboard_output_report(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
    dci: usize,
    interface_number: u8,
    value: u8,
) -> Result<usize> {
    let buf = vec![value];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    let r = xhc
        .request_set_report_bytes(
            slot,
            ctrl_ep_ring,
            dci,
            &mut buf,
            interface_number,
            2,
        )
        .await?;
    Ok(r)
}
pub async fn request_get_configuration(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
    dci: usize,
) -> Result<u8> {
    let buf = vec![0u8; 8];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_get_configuration(slot, ctrl_ep_ring, dci, &mut buf)
        .await?;
    Ok(buf[0])
}
pub async fn request_get_interface(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
    dci: usize,
    interface_number: u8,
) -> Result<u8> {
    let buf = vec![0u8; 8];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_get_interface(
        slot,
        ctrl_ep_ring,
        dci,
        interface_number,
        &mut buf,
    )
    .await?;
    Ok(buf[0])
}
pub async fn request_get_protocol(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
    dci: usize,
    interface_number: u8,
) -> Result<u8> {
    let buf = vec![0u8; 8];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_get_protocol(
        slot,
        ctrl_ep_ring,
        dci,
        interface_number,
        &mut buf,
    )
    .await?;
    Ok(buf[0])
}

pub fn pick_interface_with_triple(
    descriptors: &[UsbDescriptor],
    triple: (u8, u8, u8),
) -> Option<(ConfigDescriptor, InterfaceDescriptor, Vec<UsbDescriptor>)> {
    let mut config: Option<ConfigDescriptor> = None;
    let mut interface: Option<InterfaceDescriptor> = None;
    let mut desc_list: Vec<UsbDescriptor> = Vec::new();
    for d in descriptors {
        match d {
            UsbDescriptor::Config(e) => {
                if interface.is_some() {
                    break;
                }
                config = Some(*e);
                desc_list.clear();
            }
            UsbDescriptor::Interface(e) => {
                if triple == e.triple() {
                    interface = Some(*e)
                }
            }
            e => {
                if interface.is_some() {
                    desc_list.push(e.clone())
                }
            }
        }
    }
    if let (Some(config), Some(interface)) = (config, interface) {
        Some((config, interface, desc_list))
    } else {
        None
    }
}
pub fn descriptors_under_config(
    descriptors: &[UsbDescriptor],
    config_value: u8,
) -> Vec<UsbDescriptor> {
    let mut in_target_config = false;
    let mut desc_list: Vec<UsbDescriptor> = Vec::new();
    for d in descriptors {
        if let UsbDescriptor::Config(e) = d {
            if in_target_config {
                // Next config begins so it's end of the target config
                break;
            }
            if e.config_value() != config_value {
                // Not this config
                continue;
            }
            in_target_config = true;
        }
        if in_target_config {
            desc_list.push(d.clone())
        }
    }
    desc_list
}
pub fn descriptors_under_interface(
    descriptors: &[UsbDescriptor],
    interface_number: u8,
    alt_setting: u8,
) -> Vec<UsbDescriptor> {
    let mut in_target_interface = false;
    let mut desc_list: Vec<UsbDescriptor> = Vec::new();
    for d in descriptors {
        if let UsbDescriptor::Interface(e) = d {
            if in_target_interface {
                // Next interface begins so it's end of the target interface
                break;
            }
            if e.interface_number != interface_number
                || e.alt_setting != alt_setting
            {
                // Not this config
                continue;
            }
            in_target_interface = true;
        }
        if in_target_interface {
            desc_list.push(d.clone())
        }
    }
    desc_list
}
pub async fn request_hid_report_descriptor(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut TransferRing,
    interface_number: u8,
    desc_size: usize,
) -> Result<Vec<u8>> {
    // 7.1.1 Get_Descriptor Request
    let buf = vec![0u8; desc_size];
    let mut buf = Box::into_pin(buf.into_boxed_slice());
    xhc.request_descriptor_for_interface(
        slot,
        ctrl_ep_ring,
        UsbDescriptorType::Report,
        0,
        interface_number.into(),
        &mut buf,
    )
    .await?;
    Ok((*buf).to_vec())
}
#[derive(Debug, Copy, Clone, Default)]
#[allow(unused)]
#[repr(packed)]
pub struct HidDescriptor {
    desc_length: u8,
    desc_type: u8,
    hid_release: u16,
    country_code: u8,
    num_descriptors: u8,
    descriptor_type: u8,
    pub report_descriptor_length: u16,
}
const _: () = assert!(size_of::<HidDescriptor>() == 9);
unsafe impl Sliceable for HidDescriptor {}

pub async fn configure_endpoint(
    xhc: &Rc<Controller>,
    port: usize,
    slot: u8,
    ep_desc_list: &[EndpointDescriptor],
) -> Result<BTreeMap<usize, TransferRing>> {
    // dci == 0 : Slot Context
    // dci == 1 : ep[0] BiDir (Control Endpoint)
    // dci == 2 : ep[1] OUT
    // dci == 3 : ep[1] IN
    // dci == 4 : ep[2] OUT
    // dci == 5 : ep[2] IN
    // ...
    // dci == 31 : ep[15] IN
    let output_context = xhc.output_context_for_slot(slot)?;
    let current_slot_ctx = output_context.device_ctx.slot_ctx();

    let mut input_context = InputContext::default();
    let mut input_ctrl_ctx = InputControlContext::default();
    input_ctrl_ctx.drop_all_optional_endpoints();
    input_ctrl_ctx.add_context(0)?;
    input_context.set_slot_context(current_slot_ctx.clone());
    input_context.set_last_valid_dci(31)?;
    //input_ctrl_ctx.add_context(1)?;
    let mut ring_list = BTreeMap::new();
    for ep_desc in ep_desc_list {
        let dci = ep_desc.dci();
        input_ctrl_ctx.add_context(dci)?;
        let ep_ring = TransferRing::default();
        let ep_ctx = if ep_desc.is_interrupt_endpoint() && ep_desc.is_dir_in() {
            // [xhci] 4.8.2.4: Isoch or Interrupt Endpoints
            // EP Type = Interrupt In (7)
            // Max Packet Size = wMaxPacketSize & 0x07ff
            // Interval = ?
            // Max Burst Size =  wMaxPacketSize & 0x1800 >> 11
            // Mult = 0
            // Max ESIT Payload = ?
            // CErr = 3
            // TR Dequeue Pointer =
            // Dequeue Cycle State (DCS) = 1
            //
            // [xhci] p.70: bInterval field in USB Endpoint Descriptor
            // is in Frames (1ms) for LS/FS, or Microframes (125us) for HS/SS.
            let port_speed = xhc
                .regs
                .portsc
                .get(port)
                .ok_or("failed to get portsc")?
                .port_speed();
            let interval_time = ep_desc.calc_interval_time(port_speed)?;
            info!("interval: {interval_time:?}");
            let interval_us = interval_time.as_micros();
            let interval_value =
                (interval_us / 125).next_power_of_two().trailing_zeros() as u8;
            EndpointContext::new_interrupt_in_endpoint(
                ep_desc.max_packet_size,
                ep_ring.ring_phys_addr(),
                interval_value,
            )?
        } else if ep_desc.is_bulk_endpoint() && ep_desc.is_dir_in() {
            EndpointContext::new_bulk_in_endpoint(
                ep_desc.max_packet_size,
                ep_ring.ring_phys_addr(),
            )?
        } else {
            return Err("Unsupported ep type / dir");
        };
        info!("ep_ctx[dci={dci}]: {ep_ctx:?}");
        hexdump_struct(&ep_ctx);
        input_context.set_ep_ctx(dci, ep_ctx);
        ring_list.insert(dci, ep_ring);
    }
    input_context.set_input_ctrl_ctx(input_ctrl_ctx);
    info!("configure_endpoint: input_ctx: {input_context:?}");
    let input_context = IoBox::new(input_context);
    let cmd = GenericTrbEntry::cmd_configure_endpoint(&input_context, slot);
    xhc.send_command(cmd).await?.cmd_result_ok()?;
    info!("configure_endpoint: SUCCESS");
    Ok(ring_list)
}

pub async fn deconfigure_endpoint(
    xhc: &Rc<Controller>,
    slot: u8,
    ep_desc: &EndpointDescriptor,
) -> Result<()> {
    // dci == 1 : Control Endpoint (ep[0])
    // dci == 2 : ep[1] OUT
    // dci == 3 : ep[1] IN
    // dci == 4 : ep[2] OUT
    // dci == 5 : ep[2] IN
    // ...
    let dci = ep_desc.dci();
    let mut input_context = InputContext::default();
    {
        let mut input_ctrl_ctx = InputControlContext::default();
        input_ctrl_ctx.add_context(0)?;
        input_ctrl_ctx.drop_context(dci)?;
        input_context.set_input_ctrl_ctx(input_ctrl_ctx);
        input_context.set_last_valid_dci(dci)?;
    }
    info!("deconfigure_endpoint: dci={dci} input_ctx={input_context:?}");
    let input_context = IoBox::new(input_context);
    let cmd = GenericTrbEntry::cmd_configure_endpoint(&input_context, slot);
    xhc.send_command(cmd).await?.cmd_result_ok()?;
    Ok(())
}

pub async fn stop_endpoint(
    xhc: &Rc<Controller>,
    slot: u8,
    ep_desc: &EndpointDescriptor,
) -> Result<()> {
    // dci == 1 : Control Endpoint (ep[0])
    // dci == 2 : ep[1] OUT
    // dci == 3 : ep[1] IN
    // dci == 4 : ep[2] OUT
    // dci == 5 : ep[2] IN
    // ...
    let dci = ep_desc.dci();
    info!("stop_endpoint: dci={dci}");
    let cmd = GenericTrbEntry::cmd_stop_endpoint(slot, dci);
    xhc.send_command(cmd).await?.cmd_result_ok()?;
    Ok(())
}

pub async fn reset_endpoint(
    xhc: &Rc<Controller>,
    slot: u8,
    ep_desc: &EndpointDescriptor,
) -> Result<()> {
    // dci == 1 : Control Endpoint (ep[0])
    // dci == 2 : ep[1] OUT
    // dci == 3 : ep[1] IN
    // dci == 4 : ep[2] OUT
    // dci == 5 : ep[2] IN
    // ...
    let dci = ep_desc.dci();
    info!("reset_endpoint: dci={dci}");
    let cmd = GenericTrbEntry::cmd_reset_endpoint(slot, dci);
    xhc.send_command(cmd).await?.cmd_result_ok()?;
    Ok(())
}

pub async fn reset_device(xhc: &Rc<Controller>, slot: u8) -> Result<()> {
    info!("reset_device: slot = {slot}");
    let cmd = GenericTrbEntry::cmd_reset_device(slot);
    xhc.send_command(cmd).await?.cmd_result_ok()?;
    Ok(())
}

pub async fn print_current_ep_state(
    xhc: &Rc<Controller>,
    slot: u8,
    dci: usize,
) -> Result<()> {
    let output_context = xhc.output_context_for_slot(slot)?;
    let ep_ctx = output_context.device_ctx.ep_ctx(dci)?;
    let ep_state = ep_ctx.ep_state();
    let tr_deq_ptr = ep_ctx.tr_dequeue_ptr();
    let ep_type = ep_ctx.ep_type();
    let max_packet_size = ep_ctx.max_packet_size();
    let max_esit_payload = ep_ctx.max_esit_payload();
    let cerr = ep_ctx.error_count();
    info!(
        "EP State dci={dci}: {ep_state:?} deq={tr_deq_ptr:#018X} \
            type={ep_type:?} MaxPacketSize={max_packet_size} \
            MaxESIT Payload={max_esit_payload} CErr={cerr}"
    );
    Ok(())
}

pub async fn print_current_slot_state(
    xhc: &Rc<Controller>,
    slot: u8,
) -> Result<()> {
    let output_context = xhc.output_context_for_slot(slot)?;
    let state = output_context.device_ctx.slot_ctx().slot_state();
    let context_entries =
        output_context.device_ctx.slot_ctx().context_entries();
    info!("Slot {slot}: State={state:?}, ContextEntries={context_entries:?}");
    Ok(())
}

pub async fn print_current_portsc(
    xhc: &Rc<Controller>,
    port: usize,
) -> Result<()> {
    if let Some(portsc) = xhc.regs.portsc.get(port) {
        info!("Port {port}: {:?}", portsc);
    }
    Ok(())
}

pub trait UsbDeviceDriver {
    fn is_compatible(
        &self,
        _descriptors: &[UsbDescriptor],
        _device_descriptor: &UsbDeviceDescriptor,
    ) -> bool {
        false
    }
    fn start(
        &self,
        _xhc: Rc<Controller>,
        _port: usize,
        _slot: u8,
        _ctrl_ep_ring: TransferRing,
        _descriptors: Vec<UsbDescriptor>,
        _device_descriptor: &UsbDeviceDescriptor,
    ) {
        unimplemented!()
    }
}
