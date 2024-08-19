extern crate alloc;

use crate::error;
use crate::error::Error;
use crate::error::Result;
use crate::memory::Mmio;
use crate::mutex::Mutex;
use crate::usb::descriptor::ConfigDescriptor;
use crate::usb::descriptor::DescriptorIterator;
use crate::usb::descriptor::DescriptorType;
use crate::usb::descriptor::DeviceDescriptor;
use crate::usb::descriptor::EndpointDescriptor;
use crate::usb::descriptor::UsbDescriptor;
use crate::util::IntoPinnedMutableSlice;
use crate::xhci::context::DeviceContextBaseAddressArray;
use crate::xhci::context::EndpointContext;
use crate::xhci::context::InputContext;
use crate::xhci::context::InputControlContext;
use crate::xhci::context::OutputContext;
use crate::xhci::future::EventFuture;
use crate::xhci::registers::CapabilityRegisters;
use crate::xhci::registers::Doorbell;
use crate::xhci::registers::OperationalRegisters;
use crate::xhci::registers::PortSc;
use crate::xhci::registers::PortScIterator;
use crate::xhci::registers::PortScWrapper;
use crate::xhci::registers::RuntimeRegisters;
use crate::xhci::ring::CommandRing;
use crate::xhci::ring::EventRing;
use crate::xhci::ring::TransferRing;
use crate::xhci::trb::DataStageTrb;
use crate::xhci::trb::GenericTrbEntry;
use crate::xhci::trb::SetupStageTrb;
use crate::xhci::trb::StatusStageTrb;
use alloc::boxed::Box;
use alloc::collections::LinkedList;
use alloc::fmt::Debug;
use alloc::rc::Rc;
use alloc::rc::Weak;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::max;
use core::convert::AsRef;
use core::future::Future;
use core::mem::size_of;
use core::pin::Pin;

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
#[derive(PartialEq, Eq)]
pub enum EndpointType {
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

type DeviceFuture = Pin<Box<dyn Future<Output = Result<()>>>>;

/// Abstraction of xHCI's host controller interfaces
pub struct Controller {
    cap_regs: Mmio<CapabilityRegisters>,
    op_regs: Mmio<OperationalRegisters>,
    rt_regs: Mmio<RuntimeRegisters>,
    portsc: PortSc,
    doorbell_regs: Vec<Rc<Doorbell>>,
    command_ring: Mutex<CommandRing>,
    primary_event_ring: Mutex<EventRing>,
    device_context_base_array: Mutex<DeviceContextBaseAddressArray>,
    device_futures: Mutex<LinkedList<DeviceFuture>>,
}
impl Controller {
    pub fn new(
        cap_regs: Mmio<CapabilityRegisters>,
        op_regs: Mmio<OperationalRegisters>,
        rt_regs: Mmio<RuntimeRegisters>,
        portsc: PortSc,
        doorbell_regs: Vec<Rc<Doorbell>>,
        device_context_base_array: Mutex<DeviceContextBaseAddressArray>,
    ) -> Result<Self> {
        let mut xhc = Self {
            cap_regs,
            op_regs,
            rt_regs,
            portsc,
            doorbell_regs,
            command_ring: Mutex::new(CommandRing::default()),
            primary_event_ring: Mutex::new(EventRing::new()?),
            device_context_base_array,
            device_futures: Mutex::new(LinkedList::new()),
        };
        xhc.init_primary_event_ring()?;
        xhc.init_slots_and_contexts()?;
        xhc.init_command_ring();
        unsafe { xhc.op_regs.get_unchecked_mut() }.start_xhc();
        Ok(xhc)
    }
    pub fn device_futures(&self) -> &Mutex<LinkedList<DeviceFuture>> {
        &self.device_futures
    }
    pub fn portsc(&self, port: usize) -> Result<Weak<PortScWrapper>> {
        self.portsc.get(port)
    }
    pub fn portsc_iter(&self) -> PortScIterator {
        self.portsc.iter()
    }
    fn init_primary_event_ring(&mut self) -> Result<()> {
        let eq = &mut self.primary_event_ring;
        unsafe { self.rt_regs.get_unchecked_mut() }.init_irs(0, &mut eq.lock())
    }
    pub fn primary_event_ring(&self) -> &Mutex<EventRing> {
        &self.primary_event_ring
    }
    fn init_slots_and_contexts(&mut self) -> Result<()> {
        let num_slots = self.cap_regs.as_ref().num_of_device_slots();
        unsafe { self.op_regs.get_unchecked_mut() }.set_num_device_slots(num_slots)?;
        unsafe { self.op_regs.get_unchecked_mut() }
            .set_dcbaa_ptr(&mut self.device_context_base_array.lock())
    }
    pub fn set_output_context_for_slot(&self, slot: u8, output_context: Pin<Box<OutputContext>>) {
        self.device_context_base_array
            .lock()
            .set_output_context(slot, output_context);
    }
    fn init_command_ring(&mut self) {
        unsafe { self.op_regs.get_unchecked_mut() }.set_cmd_ring_ctrl(&self.command_ring.lock());
    }
    fn notify_xhc(&self) {
        self.doorbell_regs[0].notify(0, 0);
    }
    pub fn notify_ep(&self, slot: u8, dci: usize) -> Result<()> {
        let db = self
            .doorbell_regs
            .get(slot as usize)
            .ok_or(Error::Failed("invalid slot"))?;
        let dci = u8::try_from(dci)?;
        db.notify(dci, 0);
        Ok(())
    }
    pub async fn send_command(&self, cmd: GenericTrbEntry) -> Result<GenericTrbEntry> {
        let cmd_ptr = self.command_ring.lock().push(cmd)?;
        self.notify_xhc();
        EventFuture::new_on_trb(&self.primary_event_ring, cmd_ptr)
            .await?
            .ok_or(Error::Failed("Timed out"))
    }
    pub async fn request_initial_device_descriptor(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
    ) -> Result<DeviceDescriptor> {
        let mut desc = Box::pin(DeviceDescriptor::default());
        self.request_descriptor(
            slot,
            ctrl_ep_ring,
            DescriptorType::Device,
            0,
            0,
            desc.as_mut().as_mut_slice_sized(8)?,
        )
        .await?;
        Ok(*desc)
    }
    pub async fn request_device_descriptor(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
    ) -> Result<DeviceDescriptor> {
        let mut desc = Box::pin(DeviceDescriptor::default());
        self.request_descriptor(
            slot,
            ctrl_ep_ring,
            DescriptorType::Device,
            0,
            0,
            desc.as_mut().as_mut_slice(),
        )
        .await?;
        Ok(*desc)
    }
    pub async fn request_set_config(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
        config_value: u8,
    ) -> Result<()> {
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
        self.notify_ep(slot, 1)?;
        EventFuture::new_on_trb(&self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(Error::Failed("Timed out"))?
            .completed()
    }
    pub async fn request_set_interface(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
        interface_number: u8,
        alt_setting: u8,
    ) -> Result<()> {
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
        self.notify_ep(slot, 1)?;
        EventFuture::new_on_trb(&self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(Error::Failed("Timed out"))?
            .completed()
    }
    pub async fn request_set_protocol(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
        interface_number: u8,
        protocol: u8,
    ) -> Result<()> {
        // protocol:
        // 0: Boot Protocol
        // 1: Report Protocol
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
        self.notify_ep(slot, 1)?;
        EventFuture::new_on_trb(&self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(Error::Failed("Timed out"))?
            .completed()
    }
    pub async fn request_report_bytes(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
        buf: Pin<&mut [u8]>,
    ) -> Result<()> {
        // [HID] 7.2.1 Get_Report Request
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
        self.notify_ep(slot, 1)?;
        EventFuture::new_on_trb(&self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(Error::Failed("Timed out"))?
            .completed()
    }
    async fn request_descriptor<T: Sized>(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
        desc_type: DescriptorType,
        desc_index: u8,
        lang_id: u16,
        buf: Pin<&mut [T]>,
    ) -> Result<()> {
        ctrl_ep_ring.push(
            SetupStageTrb::new(
                SetupStageTrb::REQ_TYPE_DIR_DEVICE_TO_HOST,
                SetupStageTrb::REQ_GET_DESCRIPTOR,
                (desc_type as u16) << 8 | (desc_index as u16),
                lang_id,
                (buf.len() * size_of::<T>()) as u16,
            )
            .into(),
        )?;
        let trb_ptr_waiting = ctrl_ep_ring.push(DataStageTrb::new_in(buf).into())?;
        ctrl_ep_ring.push(StatusStageTrb::new_out().into())?;
        self.notify_ep(slot, 1)?;
        EventFuture::new_on_trb(&self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(Error::Failed("Timed out"))?
            .completed()
    }
    pub async fn request_config_descriptor_and_rest(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
    ) -> Result<Vec<UsbDescriptor>> {
        let mut config_descriptor = Box::pin(ConfigDescriptor::default());
        self.request_descriptor(
            slot,
            ctrl_ep_ring,
            DescriptorType::Config,
            0,
            0,
            config_descriptor.as_mut().as_mut_slice(),
        )
        .await?;
        let buf = vec![0; config_descriptor.total_length()];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        self.request_descriptor(
            slot,
            ctrl_ep_ring,
            DescriptorType::Config,
            0,
            0,
            buf.as_mut(),
        )
        .await?;
        let iter = DescriptorIterator::new(&buf);
        let descriptors: Vec<UsbDescriptor> = iter.collect();
        Ok(descriptors)
    }
    pub async fn request_string_descriptor(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
        lang_id: u16,
        index: u8,
    ) -> Result<String> {
        let buf = vec![0; 128];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        self.request_descriptor(
            slot,
            ctrl_ep_ring,
            DescriptorType::String,
            index,
            lang_id,
            buf.as_mut(),
        )
        .await?;
        Ok(String::from_utf8_lossy(&buf[2..])
            .to_string()
            .replace('\0', ""))
    }
    pub async fn request_string_descriptor_zero(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
    ) -> Result<Vec<u16>> {
        let buf = vec![0; 8];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        self.request_descriptor(
            slot,
            ctrl_ep_ring,
            DescriptorType::String,
            0,
            0,
            buf.as_mut(),
        )
        .await?;
        Ok(buf.as_ref().get_ref().to_vec())
    }
    pub async fn setup_endpoints(
        &self,
        port: usize,
        slot: u8,
        input_context: &mut Pin<&mut InputContext>,
        ep_desc_list: &Vec<EndpointDescriptor>,
    ) -> Result<[Option<TransferRing>; 32]> {
        // 4.6.6 Configure Endpoint
        // When configuring or deconfiguring a device, only after completing a successful
        // Configure Endpoint Command and a successful USB SET_CONFIGURATION
        // request may software schedule data transfers through a newly enabled endpoint
        // or Stream Transfer Ring of the Device Slot.
        let portsc = self
            .portsc
            .get(port)?
            .upgrade()
            .ok_or("PORTSC became invalid")?;
        let mut input_ctrl_ctx = InputControlContext::default();
        input_ctrl_ctx.add_context(0)?;
        const EP_RING_NONE: Option<TransferRing> = None;
        let mut ep_rings = [EP_RING_NONE; 32];
        let mut last_dci = 1;
        for ep_desc in ep_desc_list {
            match EndpointType::from(ep_desc) {
                EndpointType::InterruptIn => {
                    let tring = TransferRing::new(4096)?;
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
                EndpointType::BulkIn => {
                    let tring = TransferRing::new(4096)?;
                    input_ctrl_ctx.add_context(ep_desc.dci())?;
                    input_context.set_ep_ctx(
                        ep_desc.dci(),
                        EndpointContext::new_bulk_in_endpoint(
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
                EndpointType::BulkOut => {
                    let tring = TransferRing::new(4096)?;
                    input_ctrl_ctx.add_context(ep_desc.dci())?;
                    input_context.set_ep_ctx(
                        ep_desc.dci(),
                        EndpointContext::new_bulk_out_endpoint(
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
                    error!("Ignoring unimplemented ep type {:?}", ep_desc);
                }
            }
        }
        input_context.set_last_valid_dci(last_dci)?;
        input_context.set_input_ctrl_ctx(input_ctrl_ctx)?;
        let cmd = GenericTrbEntry::cmd_configure_endpoint(input_context.as_ref(), slot);
        self.send_command(cmd).await?.completed()?;
        Ok(ep_rings)
    }
    pub async fn reset_port(&self, port: usize) -> Result<()> {
        let portsc = self
            .portsc
            .get(port)?
            .upgrade()
            .ok_or("PORTSC was invalid")?;
        portsc.reset();
        Ok(())
    }
}
