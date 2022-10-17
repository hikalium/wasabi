extern crate alloc;

pub mod context;
pub mod registers;
pub mod ring;
pub mod trb;

use crate::allocator::ALLOCATOR;
use crate::arch::x86_64::paging::IoBox;
use crate::arch::x86_64::paging::Mmio;
use crate::error::Result;
use crate::error::WasabiError;
use crate::executor::yield_execution;
use crate::executor::Executor;
use crate::executor::Task;
use crate::hpet::Hpet;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::println;
use crate::usb::ConfigDescriptor;
use crate::usb::DescriptorIterator;
use crate::usb::DescriptorType;
use crate::usb::DeviceDescriptor;
use crate::usb::EndpointDescriptor;
use crate::usb::InterfaceDescriptor;
use crate::usb::IntoPinnedMutableSlice;
use crate::usb::UsbDescriptor;
use alloc::alloc::Layout;
use alloc::boxed::Box;
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
use core::pin::Pin;
use core::ptr::write_volatile;
use core::slice;
use core::task::Context;
use core::task::Poll;

use context::DeviceContextBaseAddressArray;
use context::EndpointContext;
use context::InputControlContext;
use context::RawDeviceContextBaseAddressArray;
use context::SlotContext;
use registers::CapabilityRegisters;
use registers::OperationalRegisters;
use registers::PortLinkState;
use registers::PortSc;
use registers::PortScIteratorItem;
use registers::PortState;
use registers::RuntimeRegisters;
use ring::CommandRing;
use ring::EventRing;
use ring::TransferRing;
use ring::TrbRing;
use trb::DataStageTrb;
use trb::GenericTrbEntry;
use trb::SetupStageTrb;
use trb::StatusStageTrb;
use trb::TrbType;

pub fn error_stringify<T: Debug>(x: T) -> String {
    format!("[xHC] Error: {x:?}")
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
                    EventFutureWaitType::TrbAddr(trb_addr) if trb_addr == trb.data() => {
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

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
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

pub struct Xhci {
    _bdf: BusDeviceFunction,
    cap_regs: Mmio<CapabilityRegisters>,
    op_regs: Mmio<OperationalRegisters>,
    rt_regs: Mmio<RuntimeRegisters>,
    portsc: PortSc,
    doorbell_regs: Mmio<[u32; 256]>,
    command_ring: CommandRing,
    primary_event_ring: EventRing,
    device_context_base_array: DeviceContextBaseAddressArray,
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
        cap_regs.as_ref().assert_capabilities()?;
        let mut op_regs = unsafe {
            Mmio::from_raw(bar0.addr().add(cap_regs.as_ref().length()) as *mut OperationalRegisters)
        };
        unsafe { op_regs.get_unchecked_mut() }.reset_xhc();
        let rt_regs = unsafe {
            Mmio::from_raw(bar0.addr().add(cap_regs.as_ref().rtsoff()) as *mut RuntimeRegisters)
        };
        let doorbell_regs = unsafe {
            Mmio::from_raw(bar0.addr().add(cap_regs.as_ref().dboff()) as *mut [u32; 256])
        };
        let portsc = PortSc::new(&bar0, cap_regs.as_ref());
        let scratchpad_buffers = Self::alloc_scratch_pad_buffers(
            op_regs.as_ref().page_size()?,
            cap_regs.as_ref().num_scratch_pad_bufs(),
        )?;
        let device_context_base_array = DeviceContextBaseAddressArray::new(scratchpad_buffers);
        let mut xhc = Xhci {
            _bdf: bdf,
            cap_regs,
            op_regs,
            rt_regs,
            portsc,
            doorbell_regs,
            command_ring: CommandRing::default(),
            primary_event_ring: EventRing::new()?,
            device_context_base_array,
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
        unsafe { self.rt_regs.get_unchecked_mut() }.init_irs(0, eq)?;
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
        unsafe { self.op_regs.get_unchecked_mut() }.set_cmd_ring_ctrl(&self.command_ring);
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
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring();
        ctrl_ep_ring.push(setup_stage.into())?;
        let trb_ptr_waiting = ctrl_ep_ring.push(data_stage.into())?;
        ctrl_ep_ring.push(status_stage.into())?;
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(WasabiError::Failed("Timed out"))
    }
    async fn request_set_config(&mut self, slot: u8, config_value: u8) -> Result<()> {
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring();
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
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring();
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
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring();
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
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring();
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
        let ctrl_ep_ring = &mut self.slot_context[slot as usize].ctrl_ep_ring();
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
    async fn request_string_descriptor(&mut self, slot: u8, index: u8) -> Result<String> {
        let mut buf = Vec::<u8>::new();
        buf.resize(128, 0);
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        self.request_descriptor(slot, DescriptorType::String, index, buf.as_mut())
            .await?;
        Ok(String::from_utf8_lossy(&buf[2..]).to_string())
    }
    async fn ensure_ring_is_working(&mut self) -> Result<()> {
        for _ in 0..TrbRing::NUM_TRB * 2 + 1 {
            self.send_command(GenericTrbEntry::cmd_no_op())
                .await?
                .completed()?;
        }
        Ok(())
    }
    async fn enable_slot_usb3(&mut self, port: usize) -> Result<()> {
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
        let mut input_context = slot_context.input_context();
        input_context.set_input_ctrl_ctx(input_ctrl_ctx)?;
        // 3. Initialize the Input Slot Context data structure (6.2.2)
        input_context.set_root_hub_port_number(port)?;
        input_context.set_last_valid_dci(1)?;
        // 4. Initialize the Transfer Ring for the Default Control Endpoint
        // 5. Initialize the Input default control Endpoint 0 Context (6.2.3)
        let portsc = self.portsc.get(port)?;
        let mut input_context = slot_context.input_context();
        input_context.set_port_speed(portsc.port_speed())?;
        let ctrl_ep_ring_phys_addr = slot_context.ctrl_ep_ring().ring_phys_addr();
        let mut input_context = slot_context.input_context();
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
                let slot_context = &mut self.slot_context[slot as usize];
                let mut input_context = slot_context.input_context();
                let mut input_ctrl_ctx = InputControlContext::default();
                input_ctrl_ctx.add_context(0)?;
                const EP_RING_NONE: Option<TransferRing> = None;
                let mut ep_rings = [EP_RING_NONE; 32];
                let mut last_dci = 1;
                for ep_desc in ep_desc_list {
                    match EndpointType::from(&ep_desc) {
                        EndpointType::InterruptIn => {
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
                            let transfer_trb_ptr = trb.data() as usize;
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
    async fn reset_port(&mut self, port: usize) -> Result<()> {
        let portsc = self.portsc.get(port)?;
        println!(
            "Resetting port: prev state: portsc[port = {}] = {:#10X} {:?} {:?}",
            port,
            portsc.value(),
            portsc.state(),
            portsc
        );
        portsc.reset();
        Ok(())
    }
    async fn enable_port(&mut self, port: usize) -> Result<()> {
        println!("Enabling Port {}...", port);
        // Reset port to enable the port (via Reset state)
        self.reset_port(port).await?;
        loop {
            let portsc = self.portsc.get(port)?;
            if let (PortState::Enabled, PortLinkState::U0) = (portsc.state(), portsc.pls()) {
                break;
            }
            yield_execution().await;
        }
        self.enable_slot_usb3(port).await
    }
    async fn poll(&mut self) -> Result<()> {
        let port = self
            .portsc
            .iter()
            .find_map(|PortScIteratorItem { port, portsc }| {
                if portsc.state() == PortState::Disabled {
                    Some(port)
                } else {
                    None
                }
            });
        if let Some(port) = port {
            if let Err(e) = self.enable_port(port).await {
                println!(
                    "Failed to initialize an USB device on port {}: {:?}",
                    port, e
                );
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
