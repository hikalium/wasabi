extern crate alloc;

use crate::allocator::ALLOCATOR;
use crate::bits::extract_bits;
use crate::executor::spawn_global;
use crate::executor::yield_execution;
use crate::info;
use crate::keyboard::start_usb_keyboard;
use crate::mmio::IoBox;
use crate::mmio::Mmio;
use crate::mutex::Mutex;
use crate::pci::BarMem64;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::pci::VendorDeviceId;
use crate::result::Result;
use crate::usb;
use crate::volatile::Volatile;
use crate::x86::busy_loop_hint;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::rc::Weak;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::cmp::max;
use core::future::Future;
use core::marker::PhantomPinned;
use core::mem::transmute;
use core::mem::MaybeUninit;
use core::ops::Range;
use core::pin::Pin;
use core::ptr::read_volatile;
use core::ptr::write_volatile;
use core::slice;
use core::task::Context;
use core::task::Poll;

struct XhcRegisters {
    cap_regs: Mmio<CapabilityRegisters>,
    op_regs: Mmio<OperationalRegisters>,
    rt_regs: Mmio<RuntimeRegisters>,
    doorbell_regs: Vec<Rc<Doorbell>>,
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
        let num_slots = cap_regs.as_ref().num_of_ports();
        let mut doorbell_regs = Vec::new();
        for i in 0..=num_slots {
            let ptr = unsafe { bar0.addr().add(cap_regs.as_ref().dboff()).add(4 * i) as *mut u32 };
            doorbell_regs.push(Rc::new(Doorbell::new(ptr)))
        }
        // number of doorbells will be 1 + num_slots since doorbell[] is for the host controller.
        assert!(doorbell_regs.len() == 1 + num_slots);
        Ok(XhcRegisters {
            cap_regs,
            op_regs,
            rt_regs,
            portsc,
            doorbell_regs,
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
        let mut connected_port = None;
        for port in xhc.regs.portsc.port_range() {
            if let Some(e) = xhc.regs.portsc.get(port) {
                info!("  {port:3}: {:#010X}", e.value());
                if e.ccs() {
                    connected_port = Some(port)
                }
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
        if let Some(port) = connected_port {
            info!("xhci: port {port} is connected");
            let slot = Self::init_port(&xhc, port).await?;
            info!("slot {slot} is assigned for port {port}");
            let mut ctrl_ep_ring = Self::address_device(&xhc, port, slot).await?;
            info!("AddressDeviceCommand succeeded");
            let device_descriptor =
                usb::request_device_descriptor(&xhc, slot, &mut ctrl_ep_ring).await?;
            info!("Got a DeviceDescriptor: {device_descriptor:?}");
            let vid = device_descriptor.vendor_id;
            let pid = device_descriptor.product_id;
            info!("xhci: device detected: vid:pid = {vid:#06X}:{pid:#06X}",);
            if let Ok(e) = usb::request_string_descriptor_zero(&xhc, slot, &mut ctrl_ep_ring).await
            {
                let lang_id = e[1];
                let vendor = if device_descriptor.manufacturer_idx != 0 {
                    Some(
                        usb::request_string_descriptor(
                            &xhc,
                            slot,
                            &mut ctrl_ep_ring,
                            lang_id,
                            device_descriptor.manufacturer_idx,
                        )
                        .await?,
                    )
                } else {
                    None
                };
                let product = if device_descriptor.product_idx != 0 {
                    Some(
                        usb::request_string_descriptor(
                            &xhc,
                            slot,
                            &mut ctrl_ep_ring,
                            lang_id,
                            device_descriptor.product_idx,
                        )
                        .await?,
                    )
                } else {
                    None
                };
                let serial = if device_descriptor.serial_idx != 0 {
                    Some(
                        usb::request_string_descriptor(
                            &xhc,
                            slot,
                            &mut ctrl_ep_ring,
                            lang_id,
                            device_descriptor.serial_idx,
                        )
                        .await?,
                    )
                } else {
                    None
                };
                info!("xhci: v/p/s = {vendor:?}/{product:?}/{serial:?}");
                let descriptors =
                    usb::request_config_descriptor_and_rest(&xhc, slot, &mut ctrl_ep_ring).await?;
                info!("xhci: {descriptors:?}");
                if start_usb_keyboard(&xhc, slot, &mut ctrl_ep_ring, &descriptors)
                    .await
                    .is_ok()
                {
                    return Ok(());
                }
                info!("xhci: No available drivers...");
            }
        }
        Ok(())
    }
    async fn init_port(xhc: &Rc<Controller>, port: usize) -> Result<u8> {
        let portsc = xhc.regs.portsc.get(port).ok_or("invalid portsc")?;
        info!("resetting port {port}");
        portsc.reset_port().await;
        info!("port {port} has been reset");
        portsc
            .is_enabled()
            .then_some(())
            .ok_or("port is not enabled")?;
        info!("port is enabled");
        let slot = xhc
            .send_command(GenericTrbEntry::cmd_enable_slot())
            .await?
            .slot_id();
        Ok(slot)
    }
    async fn address_device(xhc: &Rc<Controller>, port: usize, slot: u8) -> Result<CommandRing> {
        // Setup an input context and send AddressDevice command.
        // 4.3.3 Device Slot Initialization
        let output_context = Box::pin(OutputContext::default());
        xhc.set_output_context_for_slot(slot, output_context);
        let mut input_ctrl_ctx = InputControlContext::default();
        input_ctrl_ctx.add_context(0)?;
        input_ctrl_ctx.add_context(1)?;
        let mut input_context = Box::pin(InputContext::default());
        input_context.as_mut().set_input_ctrl_ctx(input_ctrl_ctx)?;
        // 3. Initialize the Input Slot Context data structure (6.2.2)
        input_context.as_mut().set_root_hub_port_number(port)?;
        input_context.as_mut().set_last_valid_dci(1)?;
        // 4. Initialize the Transfer Ring for the Default Control Endpoint
        // 5. Initialize the Input default control Endpoint 0 Context (6.2.3)
        let portsc = xhc.regs.portsc.get(port).ok_or("PORTSC was invalid")?;
        input_context.as_mut().set_port_speed(portsc.port_speed())?;
        let ctrl_ep_ring = CommandRing::default();
        input_context.as_mut().set_ep_ctx(
            1,
            EndpointContext::new_control_endpoint(
                portsc.max_packet_size()?,
                ctrl_ep_ring.ring_phys_addr(),
            )?,
        )?;
        // 8. Issue an Address Device Command for the Device Slot
        let cmd = GenericTrbEntry::cmd_address_device(input_context.as_ref(), slot);
        xhc.send_command(cmd).await?.cmd_result_ok()?;
        Ok(ctrl_ep_ring)
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
    pub fn dboff(&self) -> usize {
        self.dboff.read() as usize
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
impl EndpointContext {
    fn new() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
    fn new_control_endpoint(max_packet_size: u16, tr_dequeue_ptr: u64) -> Result<Self> {
        let mut ep = Self::new();
        ep.set_ep_type(EndpointType::Control)?;
        ep.set_dequeue_cycle_state(true)?;
        ep.set_error_count(3)?;
        ep.set_max_packet_size(max_packet_size);
        ep.set_ring_dequeue_pointer(tr_dequeue_ptr)?;
        ep.average_trb_length = 8; // 6.2.3: Software shall set Average TRB Length to ‘8’ for control endpoints.
        Ok(ep)
    }
    fn set_ring_dequeue_pointer(&mut self, tr_dequeue_ptr: u64) -> Result<()> {
        self.tr_dequeue_ptr.write_bits(4, 60, tr_dequeue_ptr >> 4)
    }
    fn set_max_packet_size(&mut self, max_packet_size: u16) {
        let max_packet_size = max_packet_size as u32;
        self.data[1] &= !(0xffff << 16);
        self.data[1] |= max_packet_size << 16;
    }
    fn set_error_count(&mut self, error_count: u32) -> Result<()> {
        if error_count & !0b11 == 0 {
            self.data[1] &= !(0b11 << 1);
            self.data[1] |= error_count << 1;
            Ok(())
        } else {
            Err("invalid error_count")
        }
    }
    fn set_dequeue_cycle_state(&mut self, dcs: bool) -> Result<()> {
        self.tr_dequeue_ptr.write_bits(0, 1, dcs.into())
    }
    fn set_ep_type(&mut self, ep_type: EndpointType) -> Result<()> {
        let raw_ep_type = ep_type as u32;
        if raw_ep_type < 8 {
            self.data[1] &= !(0b111 << 3);
            self.data[1] |= raw_ep_type << 3;
            Ok(())
        } else {
            Err("Invalid ep_type")
        }
    }
}

#[repr(C, align(32))]
#[derive(Default)]
struct DeviceContext {
    slot_ctx: [u32; 8],
    ep_ctx: [EndpointContext; 2 * 15 + 1],
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<DeviceContext>() == 0x400);
impl DeviceContext {
    fn set_port_speed(&mut self, mode: UsbMode) -> Result<()> {
        if mode.psi() < 16u32 {
            self.slot_ctx[0] &= !(0xF << 20);
            self.slot_ctx[0] |= (mode.psi()) << 20;
            Ok(())
        } else {
            Err("psi out of range")
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
            Err("num_ep_ctx out of range")
        }
    }
    fn set_root_hub_port_number(&mut self, port: usize) -> Result<()> {
        if 0 < port && port < 256 {
            self.slot_ctx[1] &= !(0xFF << 16);
            self.slot_ctx[1] |= (port as u32) << 16;
            Ok(())
        } else {
            Err("port out of range")
        }
    }
}

#[repr(C, align(4096))]
#[derive(Default)]
struct OutputContext {
    device_ctx: DeviceContext,
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<OutputContext>() <= 4096);

struct DeviceContextBaseAddressArray {
    inner: Pin<Box<RawDeviceContextBaseAddressArray>>,
    // NB: the index of context is [slot - 1], not slot.
    context: [Option<Pin<Box<OutputContext>>>; 255],
    _scratchpad_buffers: ScratchpadBuffers,
}
impl DeviceContextBaseAddressArray {
    fn new(scratchpad_buffers: ScratchpadBuffers) -> Self {
        let mut inner = RawDeviceContextBaseAddressArray::new();
        inner.scratchpad_table_ptr = scratchpad_buffers.table.as_ref().as_ptr();
        let inner = Box::pin(inner);
        Self {
            inner,
            context: unsafe { MaybeUninit::zeroed().assume_init() },
            _scratchpad_buffers: scratchpad_buffers,
        }
    }
    fn inner_mut_ptr(&mut self) -> *const RawDeviceContextBaseAddressArray {
        self.inner.as_ref().get_ref() as *const RawDeviceContextBaseAddressArray
    }
    fn set_output_context(&mut self, slot: u8, output_context: Pin<Box<OutputContext>>) {
        let ctx_idx = slot as usize - 1;
        // Own the output context here
        self.context[ctx_idx] = Some(output_context);
        // ...and set it in the actual pointer array
        unsafe {
            self.inner.as_mut().get_unchecked_mut().context[ctx_idx] =
                self.context[ctx_idx]
                    .as_ref()
                    .expect("Output Context was None")
                    .as_ref()
                    .get_ref() as *const OutputContext as u64;
        }
    }
}

pub struct Controller {
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
    async fn send_command(&self, cmd: GenericTrbEntry) -> Result<GenericTrbEntry> {
        let cmd_ptr = self.command_ring.lock().push(cmd)?;
        self.notify_xhc();
        EventFuture::new_for_trb(&self.primary_event_ring, cmd_ptr).await
    }
    fn notify_xhc(&self) {
        self.regs.doorbell_regs[0].notify(0, 0);
    }
    pub fn notify_ep(&self, slot: u8, dci: usize) -> Result<()> {
        let db = self
            .regs
            .doorbell_regs
            .get(slot as usize)
            .ok_or("invalid slot")?;
        let dci = u8::try_from(dci).or(Err("invalid dci"))?;
        db.notify(dci, 0);
        Ok(())
    }
    fn set_output_context_for_slot(&self, slot: u8, output_context: Pin<Box<OutputContext>>) {
        self.device_context_base_array
            .lock()
            .set_output_context(slot, output_context);
    }
    pub async fn request_descriptor<T: Sized>(
        &self,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
        desc_type: usb::UsbDescriptorType,
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
        EventFuture::new_for_trb(&self.primary_event_ring, trb_ptr_waiting)
            .await?
            .transfer_result_ok()
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
        EventFuture::new_for_trb(&self.primary_event_ring, trb_ptr_waiting)
            .await?
            .transfer_result_ok()
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
        EventFuture::new_for_trb(&self.primary_event_ring, trb_ptr_waiting)
            .await?
            .transfer_result_ok()
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
        EventFuture::new_for_trb(&self.primary_event_ring, trb_ptr_waiting)
            .await?
            .transfer_result_ok()
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
        EventFuture::new_for_trb(&self.primary_event_ring, trb_ptr_waiting)
            .await?
            .transfer_result_ok()
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
    pub fn register_waiter(&mut self, wait: &Rc<EventWaitInfo>) {
        let wait = Rc::downgrade(wait);
        self.wait_list.push_back(wait);
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
    fn advance_index(&mut self, new_cycle: bool) -> Result<()> {
        if self.current().cycle_state() == new_cycle {
            return Err("cycle state does not change");
        }
        self.trb[self.current_index].set_cycle_state(new_cycle);
        self.current_index = (self.current_index + 1) % self.trb.len();
        Ok(())
    }
    fn write_current(&mut self, trb: GenericTrbEntry) {
        self.write(self.current_index, trb)
            .expect("writing to the current index shall not fail")
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
    const CTRL_BIT_INTERRUPT_ON_SHORT_PACKET: u32 = 1 << 2;
    const CTRL_BIT_INTERRUPT_ON_COMPLETION: u32 = 1 << 5;
    const CTRL_BIT_IMMEDIATE_DATA: u32 = 1 << 6;
    const CTRL_BIT_DATA_DIR_IN: u32 = 1 << 16;
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
    pub fn set_cycle_state(&mut self, cycle: bool) {
        self.control.write_bits(0, 1, cycle.into()).unwrap()
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
    pub fn cmd_enable_slot() -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::EnableSlotCommand);
        trb
    }
    pub fn completion_code(&self) -> u32 {
        self.option.read_bits(24, 8)
    }
    fn cmd_result_ok(&self) -> Result<()> {
        if self.trb_type() != TrbType::CommandCompletionEvent as u32 {
            Err("Not a CommandCompletionEvent")
        } else if self.completion_code() != 1 {
            info!(
                "Completion code was not Success. actual = {}",
                self.completion_code()
            );
            Err("CompletionCode was not Success")
        } else {
            Ok(())
        }
    }
    fn transfer_result_ok(&self) -> Result<()> {
        if self.trb_type() != TrbType::TransferEvent as u32 {
            Err("Not a TransferEvent")
        } else if self.completion_code() != 1 && self.completion_code() != 13 {
            info!(
                "Transfer failed. Actual CompletionCode = {}",
                self.completion_code()
            );
            Err("CompletionCode was not Success")
        } else {
            Ok(())
        }
    }
    fn set_slot_id(&mut self, slot: u8) {
        self.control.write_bits(24, 8, slot as u32).unwrap()
    }
    fn cmd_address_device(input_context: Pin<&InputContext>, slot: u8) -> Self {
        let mut trb = Self::default();
        trb.set_trb_type(TrbType::AddressDeviceCommand);
        trb.data
            .write(input_context.get_ref() as *const InputContext as u64);
        trb.set_slot_id(slot);
        trb
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

pub struct CommandRing {
    ring: IoBox<TrbRing>,
    cycle_state_ours: bool,
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
            return Err("Command Ring is Full");
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

#[derive(Debug, Default)]
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
    fn bit(&self, pos: usize) -> bool {
        (self.value() & (1 << pos)) != 0
    }
    fn ccs(&self) -> bool {
        // CCS - Current Connect Status - ROS
        self.bit(0)
    }
    fn assert_bit(&self, pos: usize) {
        const PRESERVE_MASK: u32 = 0b01001111000000011111111111101001;
        let portsc = self.ptr.lock();
        let old = unsafe { read_volatile(*portsc) };
        unsafe { write_volatile(*portsc, (old & PRESERVE_MASK) | (1 << pos)) }
    }
    fn pp(&self) -> bool {
        // PP - Port Power - RWS
        self.bit(9)
    }
    fn assert_pp(&self) {
        // PP - Port Power - RWS
        self.assert_bit(9)
    }
    pub fn pr(&self) -> bool {
        // PR - Port Reset - RW1S
        self.bit(4)
    }
    pub fn assert_pr(&self) {
        // PR - Port Reset - RW1S
        self.assert_bit(4)
    }
    pub async fn reset_port(&self) {
        self.assert_pp();
        while !self.pp() {
            yield_execution().await
        }
        self.assert_pr();
        while self.pr() {
            yield_execution().await
        }
    }
    pub fn ped(&self) -> bool {
        // PED - Port Enabled/Disabled - RW1CS
        self.bit(1)
    }
    pub fn is_enabled(&self) -> bool {
        self.pp() && self.ccs() && self.ped() && !self.pr()
    }
    pub fn max_packet_size(&self) -> Result<u16> {
        match self.port_speed() {
            UsbMode::FullSpeed | UsbMode::LowSpeed => Ok(8),
            UsbMode::HighSpeed => Ok(64),
            UsbMode::SuperSpeed => Ok(512),
            _ => Err("Unknown Protocol Speeed ID"),
        }
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
}

// [xhci] 4.7 Doorbells
// index 0: for the host controller
// index 1-255: for device contexts (index by a Slot ID)
// DO NOT implement Copy trait - this should be the only instance to have the ptr.
pub struct Doorbell {
    ptr: Mutex<*mut u32>,
}
impl Doorbell {
    pub fn new(ptr: *mut u32) -> Self {
        Self {
            ptr: Mutex::new(ptr),
        }
    }
    // [xhci] 5.6 Doorbell Registers
    // bit 0..8: DB Target
    // bit 8..16: RsvdZ
    // bit 16..32: DB Task ID
    // index 0: for the host controller
    // index 1-255: for device contexts (index by a Slot ID)
    pub fn notify(&self, target: u8, task: u16) {
        let value = (target as u32) | (task as u32) << 16;
        // SAFETY: This is safe as long as the ptr is valid
        unsafe {
            write_volatile(*self.ptr.lock(), value);
        }
    }
}
#[derive(Clone)]
struct EventFuture {
    wait_on: Rc<EventWaitInfo>,
    _pinned: PhantomPinned,
}
impl EventFuture {
    fn new(event_ring: &Mutex<EventRing>, cond: EventWaitCond) -> Self {
        let wait_on = EventWaitInfo {
            cond,
            trbs: Default::default(),
        };
        let wait_on = Rc::new(wait_on);
        event_ring.lock().register_waiter(&wait_on);
        Self {
            wait_on,
            _pinned: PhantomPinned,
        }
    }
    fn new_for_trb(event_ring: &Mutex<EventRing>, trb_addr: u64) -> Self {
        let trb_addr = Some(trb_addr);
        Self::new(
            event_ring,
            EventWaitCond {
                trb_addr,
                ..Default::default()
            },
        )
    }
}
impl Future for EventFuture {
    type Output = Result<GenericTrbEntry>;
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<GenericTrbEntry>> {
        let mut_self = unsafe { self.get_unchecked_mut() };
        if let Some(trb) = mut_self.wait_on.trbs.lock().pop_front() {
            Poll::Ready(Ok(trb))
        } else {
            Poll::Pending
        }
    }
}

#[repr(C, align(32))]
#[derive(Default)]
pub struct InputControlContext {
    drop_context_bitmap: u32,
    add_context_bitmap: u32,
    data: [u32; 6],
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<InputControlContext>() == 0x20);
impl InputControlContext {
    pub fn add_context(&mut self, ici: usize) -> Result<()> {
        if ici < 32 {
            self.add_context_bitmap |= 1 << ici;
            Ok(())
        } else {
            Err("Input context index out of range")
        }
    }
}

#[repr(C, align(4096))]
#[derive(Default)]
pub struct InputContext {
    input_ctrl_ctx: InputControlContext,
    device_ctx: DeviceContext,
    //
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<InputContext>() <= 4096);
impl InputContext {
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
    fn set_port_speed(self: &mut Pin<&mut Self>, psi: UsbMode) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut() }
            .device_ctx
            .set_port_speed(psi)
    }
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
    pub const REQ_TYPE_DIR_HOST_TO_DEVICE: u8 = 0 << 7;
    // bmRequest bit[5..=6]: Request Type
    //      0: Standard
    //      1: Class
    //      2: Vendor
    //      _: Reserved
    //pub const REQ_TYPE_TYPE_STANDARD: u8 = 0 << 5;
    pub const REQ_TYPE_TYPE_CLASS: u8 = 1 << 5;
    pub const REQ_TYPE_TYPE_VENDOR: u8 = 2 << 5;
    // bmRequest bit[0..=4]: Recipient
    //      0: Device
    //      1: Interface
    //      2: Endpoint
    //      3: Other
    //      _: Reserved
    pub const REQ_TYPE_TO_DEVICE: u8 = 0;
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
