extern crate alloc;

pub mod context;
pub mod future;
pub mod registers;
pub mod ring;
pub mod trb;

use crate::allocator::ALLOCATOR;
use crate::arch::x86_64::paging::IoBox;
use crate::arch::x86_64::paging::Mmio;
use crate::ax88179;
use crate::error::Error;
use crate::error::Result;
use crate::executor::yield_execution;
use crate::executor::Task;
use crate::executor::ROOT_EXECUTOR;
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
use crate::usb_hid_keyboard;
use crate::util::PAGE_SIZE;
use alloc::alloc::Layout;
use alloc::boxed::Box;
use alloc::fmt::Debug;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::cmp::max;
use core::convert::AsRef;
use core::mem::size_of;
use core::pin::Pin;
use core::ptr::write_volatile;
use core::slice;

use context::DeviceContextBaseAddressArray;
use context::EndpointContext;
use context::InputContext;
use context::InputControlContext;
use context::OutputContext;
use context::RawDeviceContextBaseAddressArray;
use future::CommandCompletionEventFuture;
use future::TransferEventFuture;
use registers::CapabilityRegisters;
use registers::OperationalRegisters;
use registers::PortLinkState;
use registers::PortSc;
use registers::PortScIteratorItem;
use registers::PortScWrapper;
use registers::PortState;
use registers::RuntimeRegisters;
use registers::UsbMode;
use ring::CommandRing;
use ring::EventRing;
use ring::TransferRing;
use ring::TrbRing;
use trb::DataStageTrb;
use trb::GenericTrbEntry;
use trb::SetupStageTrb;
use trb::StatusStageTrb;

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
    fn attach(&self, bdf: BusDeviceFunction) -> Result<Box<dyn PciDeviceDriverInstance>> {
        Ok(Box::new(XhciDriverInstance::new(bdf)?) as Box<dyn PciDeviceDriverInstance>)
    }
    fn name(&self) -> &str {
        "XhciDriver"
    }
}

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
        op_regs.as_ref().assert_params()?;
        let doorbell_regs = unsafe {
            Mmio::from_raw(bar0.addr().add(cap_regs.as_ref().dboff()) as *mut [u32; 256])
        };
        let portsc = PortSc::new(&bar0, cap_regs.as_ref());
        let scratchpad_buffers =
            Self::alloc_scratch_pad_buffers(cap_regs.as_ref().num_scratch_pad_bufs())?;
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
        };
        xhc.init_primary_event_ring()?;
        xhc.init_slots_and_contexts()?;
        xhc.init_command_ring()?;
        unsafe { xhc.op_regs.get_unchecked_mut() }.start_xhc();

        Ok(xhc)
    }
    pub fn portsc(&mut self, port: usize) -> Result<PortScWrapper> {
        self.portsc.get(port)
    }
    fn init_primary_event_ring(&mut self) -> Result<()> {
        let eq = &mut self.primary_event_ring;
        unsafe { self.rt_regs.get_unchecked_mut() }.init_irs(0, eq)?;
        Ok(())
    }
    pub fn primary_event_ring(&mut self) -> &mut EventRing {
        &mut self.primary_event_ring
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
    fn alloc_scratch_pad_buffers(num_scratch_pad_bufs: usize) -> Result<Pin<Box<[*mut u8]>>> {
        // 4.20 Scratchpad Buffers
        // This should be done before xHC starts.
        // device_contexts.context[0] points Scratchpad Buffer Arrary.
        // The array contains pointers to a memory region which is sized PAGESIZE and aligned on
        // PAGESIZE. (PAGESIZE can be retrieved from op_regs.PAGESIZE)
        let scratchpad_buffers = ALLOCATOR.alloc_with_options(
            Layout::from_size_align(size_of::<usize>() * num_scratch_pad_bufs, PAGE_SIZE)
                .map_err(|_| Error::Failed("could not allocated scratchpad buffers"))?,
        );
        let scratchpad_buffers = unsafe {
            slice::from_raw_parts(scratchpad_buffers as *mut *mut u8, num_scratch_pad_bufs)
        };
        let mut scratchpad_buffers = Pin::new(Box::<[*mut u8]>::from(scratchpad_buffers));
        for sb in scratchpad_buffers.iter_mut() {
            *sb = ALLOCATOR.alloc_with_options(
                Layout::from_size_align(PAGE_SIZE, PAGE_SIZE)
                    .map_err(|_| Error::Failed("could not allocated scratchpad buffers"))?,
            );
        }
        Ok(scratchpad_buffers)
    }
    fn notify_xhc(&mut self) {
        unsafe {
            write_volatile(&mut self.doorbell_regs.get_unchecked_mut()[0], 0);
        }
    }
    pub fn notify_ep(&mut self, slot: u8, dci: usize) {
        unsafe {
            write_volatile(
                &mut self.doorbell_regs.get_unchecked_mut()[slot as usize],
                dci as u32,
            );
        }
    }
    pub async fn send_command(&mut self, cmd: GenericTrbEntry) -> Result<GenericTrbEntry> {
        let cmd_ptr = self.command_ring.push(cmd)?;
        self.notify_xhc();
        CommandCompletionEventFuture::new(&mut self.primary_event_ring, cmd_ptr)
            .await?
            .ok_or(Error::Failed("Timed out"))
    }
    async fn request_initial_device_descriptor(
        &mut self,
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
    async fn request_device_descriptor(
        &mut self,
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
        &mut self,
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
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(Error::Failed("Timed out"))?
            .completed()
    }
    pub async fn request_set_interface(
        &mut self,
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
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(Error::Failed("Timed out"))?
            .completed()
    }
    pub async fn request_set_protocol(
        &mut self,
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
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(Error::Failed("Timed out"))?
            .completed()
    }
    pub async fn request_report_bytes(
        &mut self,
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
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(Error::Failed("Timed out"))?
            .completed()
    }
    async fn request_descriptor<T: Sized>(
        &mut self,
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
        self.notify_ep(slot, 1);
        TransferEventFuture::new(&mut self.primary_event_ring, trb_ptr_waiting)
            .await?
            .ok_or(Error::Failed("Timed out"))?
            .completed()
    }
    async fn request_config_descriptor_and_rest(
        &mut self,
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
        let mut buf = Vec::<u8>::new();
        buf.resize(config_descriptor.total_length(), 0);
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
    async fn request_string_descriptor(
        &mut self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
        lang_id: u16,
        index: u8,
    ) -> Result<String> {
        let mut buf = Vec::<u8>::new();
        buf.resize(128, 0);
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
        Ok(String::from_utf8_lossy(&buf[2..]).to_string())
    }
    async fn request_string_descriptor_zero(
        &mut self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
    ) -> Result<Vec<u16>> {
        let mut buf = Vec::<u16>::new();
        buf.resize(8, 0);
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
    async fn ensure_ring_is_working(&mut self) -> Result<()> {
        for _ in 0..TrbRing::NUM_TRB * 2 + 1 {
            self.send_command(GenericTrbEntry::cmd_no_op())
                .await?
                .completed()?;
        }
        Ok(())
    }
    pub async fn setup_endpoints(
        &mut self,
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
        let portsc = self.portsc.get(port)?;
        let mut input_ctrl_ctx = InputControlContext::default();
        input_ctrl_ctx.add_context(0)?;
        const EP_RING_NONE: Option<TransferRing> = None;
        let mut ep_rings = [EP_RING_NONE; 32];
        let mut last_dci = 1;
        for ep_desc in ep_desc_list {
            match EndpointType::from(ep_desc) {
                EndpointType::InterruptIn => {
                    println!("InterruptIn! dci={}: {:?}", ep_desc.dci(), ep_desc);
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
                    println!("BulkIn! dci={}: {:?}", ep_desc.dci(), ep_desc);
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
                    println!("BulkOut! dci={}: {:?}", ep_desc.dci(), ep_desc);
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
                    println!("Ignoring {:?}", ep_desc);
                }
            }
        }
        input_context.set_last_valid_dci(last_dci)?;
        input_context.set_input_ctrl_ctx(input_ctrl_ctx)?;
        let cmd = GenericTrbEntry::cmd_configure_endpoint(input_context.as_ref(), slot);
        self.send_command(cmd).await?.completed()?;
        println!("Endpoint setup done!");
        Ok(ep_rings)
    }
    async fn device_ready(
        &mut self,
        port: usize,
        slot: u8,
        input_context: &mut Pin<&mut InputContext>,
        ctrl_ep_ring: &mut CommandRing,
    ) -> Result<()> {
        let portsc = self.portsc.get(port)?;
        println!("Port speed: {:?}", portsc.port_speed());
        if portsc.port_speed() == UsbMode::FullSpeed {
            // For full speed device, we should read the first 8 bytes of the device descriptor to
            // get proper MaxPacketSize parameter.
            let device_descriptor = self
                .request_initial_device_descriptor(slot, ctrl_ep_ring)
                .await?;
            let max_packet_size = device_descriptor.max_packet_size;
            println!(
                "Updating MaxPacketSize to {} for FullSpeed device",
                max_packet_size
            );
            let mut input_ctrl_ctx = InputControlContext::default();
            input_ctrl_ctx.add_context(0)?;
            input_ctrl_ctx.add_context(1)?;
            input_context.set_input_ctrl_ctx(input_ctrl_ctx)?;
            input_context.set_ep_ctx(
                1,
                EndpointContext::new_control_endpoint(
                    max_packet_size as u16,
                    ctrl_ep_ring.ring_phys_addr(),
                )?,
            )?;
            let cmd = GenericTrbEntry::cmd_evaluate_context(input_context.as_ref(), slot);
            self.send_command(cmd).await?.completed()?;
            println!("Evaluated");
            //ctrl_ep_ring.reset();
        }
        let device_descriptor = self.request_device_descriptor(slot, ctrl_ep_ring).await?;
        println!("{:?}", device_descriptor);
        let descriptors = self
            .request_config_descriptor_and_rest(slot, ctrl_ep_ring)
            .await?;
        for d in &descriptors {
            println!("{:?}", d);
        }
        if let Ok(e) = self
            .request_string_descriptor_zero(slot, ctrl_ep_ring)
            .await
        {
            println!("String Descriptor Zero: {:?}", e);
            let lang_id = e[1];
            println!("Using lang_id {:#06X}", lang_id);
            if device_descriptor.manufacturer_idx != 0 {
                let vendor_name = self
                    .request_string_descriptor(
                        slot,
                        ctrl_ep_ring,
                        lang_id,
                        device_descriptor.manufacturer_idx,
                    )
                    .await?;
                println!("Vendor: {}", vendor_name);
            } else {
                println!("Vendor is not available");
            }
            if device_descriptor.product_idx != 0 {
                let product_name = self
                    .request_string_descriptor(
                        slot,
                        ctrl_ep_ring,
                        lang_id,
                        device_descriptor.product_idx,
                    )
                    .await?;
                println!("Product: {}", product_name);
            } else {
                println!("Product is not available");
            }
            if device_descriptor.serial_idx != 0 {
                let serial = self
                    .request_string_descriptor(
                        slot,
                        ctrl_ep_ring,
                        lang_id,
                        device_descriptor.serial_idx,
                    )
                    .await?;
                println!("Serial: {}", serial);
            } else {
                println!("Serial is not available");
            }
        }
        if device_descriptor.vendor_id == 2965 && device_descriptor.product_id == 6032 {
            println!("AX88179!");
            ax88179::attach_usb_device(self, port, slot, input_context, ctrl_ep_ring, &descriptors)
                .await?;
        } else if device_descriptor.vendor_id == 0x0bda
            && (device_descriptor.product_id == 0x8153 || device_descriptor.product_id == 0x8151)
        {
            println!("rtl8153/8151 is not supported yet...");
        } else if device_descriptor.device_class == 0 {
            // Device class is derived from Interface Descriptor
            let mut boot_keyboard_interface: Option<InterfaceDescriptor> = None;
            for d in &descriptors {
                if let UsbDescriptor::Interface(e) = d {
                    if let (3, 1, 1) = e.triple() {
                        boot_keyboard_interface = Some(*e)
                    }
                }
            }
            if boot_keyboard_interface.is_some() {
                println!("!!!!! USB KBD Found!");
                /*
                usb_hid_keyboard::attach_usb_device(
                    self,
                    port,
                    slot,
                    input_context,
                    ctrl_ep_ring,
                    &descriptors,
                )
                .await?;
                */
            }
        } else {
            println!(
                "Device class {} is not supported yet",
                device_descriptor.device_class
            );
        }
        Ok(())
    }
    async fn address_device(&mut self, port: usize, slot: u8) -> Result<()> {
        // Setup an input context and send AddressDevice command.
        // 4.3.3 Device Slot Initialization
        let output_context = Box::pin(OutputContext::default());
        self.device_context_base_array
            .set_output_context(slot, output_context);
        let mut input_ctrl_ctx = InputControlContext::default();
        input_ctrl_ctx.add_context(0)?;
        input_ctrl_ctx.add_context(1)?;
        let mut input_context = Box::pin(InputContext::default());
        let mut input_context = input_context.as_mut();
        input_context.set_input_ctrl_ctx(input_ctrl_ctx)?;
        // 3. Initialize the Input Slot Context data structure (6.2.2)
        input_context.set_root_hub_port_number(port)?;
        input_context.set_last_valid_dci(1)?;
        // 4. Initialize the Transfer Ring for the Default Control Endpoint
        // 5. Initialize the Input default control Endpoint 0 Context (6.2.3)
        let portsc = self.portsc.get(port)?;
        input_context.set_port_speed(portsc.port_speed())?;
        let mut ctrl_ep_ring = Box::pin(CommandRing::default());
        let mut ctrl_ep_ring = ctrl_ep_ring.as_mut();
        input_context.set_ep_ctx(
            1,
            EndpointContext::new_control_endpoint(
                portsc.max_packet_size()?,
                ctrl_ep_ring.ring_phys_addr(),
            )?,
        )?;
        // 8. Issue an Address Device Command for the Device Slot
        let cmd = GenericTrbEntry::cmd_address_device(input_context.as_ref(), slot);
        println!("Sending AddressDeviceCommand...");
        self.send_command(cmd).await?.completed()?;
        println!("Device Addressed!");
        self.device_ready(port, slot, &mut input_context, &mut ctrl_ep_ring)
            .await
    }
    async fn enable_slot(&mut self, port: usize) -> Result<()> {
        let portsc = self.portsc.get(port)?;
        if !portsc.ccs() {
            return Err(Error::FailedString(format!(
                "port {} disconnected while initialization",
                port
            )));
        }
        let slot = self
            .send_command(GenericTrbEntry::cmd_enable_slot())
            .await?
            .slot_id();
        println!("USB Device Detected, port = {}, slot {}", port, slot);
        self.address_device(port, slot).await
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
        self.enable_slot(port).await
    }
    async fn poll(&mut self) -> Result<()> {
        // 4.3 USB Device Initialization
        // USB3: Disconnected -> Polling -> Enabled
        // USB2: Disconnected -> Disabled
        if let Some((port, portsc)) =
            self.portsc
                .iter()
                .rev()
                .find_map(|PortScIteratorItem { port, portsc }| {
                    if portsc.csc() {
                        portsc.clear_csc();
                        Some((port, portsc))
                    } else {
                        None
                    }
                })
        {
            if portsc.ccs() {
                println!("Port {}: Device attached: {:?}", port, portsc);
                if portsc.state() == PortState::Disabled {
                    // USB2
                    if let Err(e) = self.enable_port(port).await {
                        println!(
                            "Failed to initialize an USB2 device on port {}: {:?}",
                            port, e
                        );
                    }
                } else if portsc.state() == PortState::Enabled {
                    // USB3
                    if let Err(e) = self.enable_slot(port).await {
                        println!(
                            "Failed to initialize an USB3 device on port {}: {:?}",
                            port, e
                        );
                    }
                }
            } else {
                println!("Port {}: Device detached: {:?}", port, portsc);
            }
        }
        Ok(())
    }
}

struct XhciDriverInstance {}
impl XhciDriverInstance {
    fn new(bdf: BusDeviceFunction) -> Result<Self> {
        (*ROOT_EXECUTOR.lock()).spawn(Task::new(async move {
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
