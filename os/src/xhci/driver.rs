extern crate alloc;

use crate::ax88179;
use crate::error;
use crate::error::Error;
use crate::error::Result;
use crate::executor::dummy_waker;
use crate::executor::spawn_global;
use crate::executor::yield_execution;
use crate::info;
use crate::pci::BusDeviceFunction;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::usb::descriptor::UsbDescriptor;
use crate::usb_hid_keyboard;
use crate::usb_hid_tablet;
use crate::warn;
use crate::xhci::context::EndpointContext;
use crate::xhci::context::InputContext;
use crate::xhci::context::InputControlContext;
use crate::xhci::context::OutputContext;
use crate::xhci::controller::Controller;
use crate::xhci::device::UsbDeviceDriverContext;
use crate::xhci::init::create_host_controller;
use crate::xhci::registers::PortLinkState;
use crate::xhci::registers::PortScIteratorItem;
use crate::xhci::registers::PortScWrapper;
use crate::xhci::registers::PortState;
use crate::xhci::registers::UsbMode;
use crate::xhci::ring::CommandRing;
use crate::xhci::ring::TrbRing;
use crate::xhci::trb::GenericTrbEntry;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use core::future::Future;
use core::pin::Pin;
use core::task::Context;

#[derive(Default)]
pub struct XhciDriverForPci {}
impl XhciDriverForPci {
    async fn update_max_packet_size(
        xhc: &Rc<Controller>,
        port: usize,
        slot: u8,
        input_context: &mut Pin<Box<InputContext>>,
        ctrl_ep_ring: &mut Pin<Box<CommandRing>>,
    ) -> Result<()> {
        if xhc
            .portsc(port)?
            .upgrade()
            .ok_or("PORTSC was invalid")?
            .port_speed()
            != UsbMode::FullSpeed
        {
            return Ok(());
        }
        // TODO: refactor this part out
        // For full speed device, we should read the first 8 bytes of the device descriptor to
        // get proper MaxPacketSize parameter.
        let device_descriptor = xhc
            .request_initial_device_descriptor(slot, ctrl_ep_ring)
            .await?;
        let max_packet_size = device_descriptor.max_packet_size;
        let mut input_ctrl_ctx = InputControlContext::default();
        input_ctrl_ctx.add_context(0)?;
        input_ctrl_ctx.add_context(1)?;
        input_context.as_mut().set_input_ctrl_ctx(input_ctrl_ctx)?;
        input_context.as_mut().set_ep_ctx(
            1,
            EndpointContext::new_control_endpoint(
                max_packet_size as u16,
                ctrl_ep_ring.ring_phys_addr(),
            )?,
        )?;
        let cmd = GenericTrbEntry::cmd_evaluate_context(input_context.as_ref(), slot);
        xhc.send_command(cmd).await?.completed()
    }
    async fn device_ready(
        xhc: Rc<Controller>,
        port: usize,
        slot: u8,
        mut input_context: Pin<Box<InputContext>>,
        mut ctrl_ep_ring: Pin<Box<CommandRing>>,
    ) -> Result<Pin<Box<dyn Future<Output = Result<()>>>>> {
        Self::update_max_packet_size(&xhc, port, slot, &mut input_context, &mut ctrl_ep_ring)
            .await?;
        let device_descriptor = xhc
            .request_device_descriptor(slot, &mut ctrl_ep_ring)
            .await?;
        let descriptors = xhc
            .request_config_descriptor_and_rest(slot, &mut ctrl_ep_ring)
            .await?;
        let device_vendor_id = device_descriptor.vendor_id;
        let device_product_id = device_descriptor.product_id;
        if let Ok(e) = xhc
            .request_string_descriptor_zero(slot, &mut ctrl_ep_ring)
            .await
        {
            let lang_id = e[1];
            let vendor = if device_descriptor.manufacturer_idx != 0 {
                Some(
                    xhc.request_string_descriptor(
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
                    xhc.request_string_descriptor(
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
                    xhc.request_string_descriptor(
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
            info!("USB device detected: vendor/product/serial =  {vendor:?}/{product:?}/{serial:?} (vid:pid = {device_vendor_id:#06X}:{device_product_id:#06X})");
        } else {
            info!(
                "USB device detected: vid:pid = {device_vendor_id:#06X}:{device_product_id:#06X}",
            );
        }
        let ddc =
            UsbDeviceDriverContext::new(port, slot, xhc, input_context, ctrl_ep_ring, descriptors)
                .await?;
        if device_vendor_id == 2965 && device_product_id == 6032 {
            ax88179::attach_usb_device(ddc).await?;
        } else if device_vendor_id == 0x0bda
            && (device_product_id == 0x8153 || device_product_id == 0x8151)
        {
            error!("rtl8153/8151 is not supported yet...");
        } else if device_descriptor.device_class == 0 {
            // Device class is derived from Interface Descriptor
            for d in ddc.descriptors() {
                if let UsbDescriptor::Interface(e) = d {
                    match e.triple() {
                        (3, 0, 0) => {
                            let f = usb_hid_tablet::attach_usb_device(ddc);
                            return Ok(Box::pin(f));
                        }
                        (3, 1, 1) => {
                            let f = usb_hid_keyboard::attach_usb_device(ddc);
                            return Ok(Box::pin(f));
                        }
                        triple => warn!("Skipping unknown interface triple: {triple:?}"),
                    }
                }
            }
        }
        Err(Error::FailedString(format!(
            "Device class {} is not supported yet",
            device_descriptor.device_class
        )))
    }
    async fn address_device(
        xhc: Rc<Controller>,
        port: usize,
        slot: u8,
    ) -> Result<Pin<Box<dyn Future<Output = Result<()>>>>> {
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
        let portsc = xhc.portsc(port)?.upgrade().ok_or("PORTSC was invalid")?;
        input_context.as_mut().set_port_speed(portsc.port_speed())?;
        let mut ctrl_ep_ring = Box::pin(CommandRing::default());
        input_context.as_mut().set_ep_ctx(
            1,
            EndpointContext::new_control_endpoint(
                portsc.max_packet_size()?,
                ctrl_ep_ring.as_mut().ring_phys_addr(),
            )?,
        )?;
        // 8. Issue an Address Device Command for the Device Slot
        let cmd = GenericTrbEntry::cmd_address_device(input_context.as_ref(), slot);
        xhc.send_command(cmd).await?.completed()?;
        Self::device_ready(xhc.clone(), port, slot, input_context, ctrl_ep_ring).await
    }
    async fn ensure_ring_is_working(xhc: Rc<Controller>) -> Result<()> {
        for _ in 0..TrbRing::NUM_TRB * 2 + 1 {
            xhc.send_command(GenericTrbEntry::cmd_no_op())
                .await?
                .completed()?;
        }
        Ok(())
    }
    async fn enable_slot(
        xhc: Rc<Controller>,
        port: usize,
    ) -> Result<Pin<Box<dyn Future<Output = Result<()>>>>> {
        let portsc = xhc.portsc(port)?.upgrade().ok_or("PORTSC was invalid")?;
        if !portsc.ccs() {
            return Err(Error::FailedString(format!(
                "port {} disconnected while initialization",
                port
            )));
        }
        let slot = xhc
            .send_command(GenericTrbEntry::cmd_enable_slot())
            .await?
            .slot_id();
        Self::address_device(xhc.clone(), port, slot).await
    }
    /// Returns a future that handles device disconnect when needed.
    async fn enable_port(
        xhc: Rc<Controller>,
        port: usize,
    ) -> Result<Pin<Box<dyn Future<Output = Result<()>>>>> {
        // Reset port to enable the port (via Reset state)
        xhc.reset_port(port).await?;
        loop {
            let portsc = xhc.portsc(port)?.upgrade().ok_or("PORTSC was invalid")?;
            if let (PortState::Enabled, PortLinkState::U0) = (portsc.state(), portsc.pls()) {
                break;
            }
            yield_execution().await;
        }
        Self::enable_slot(xhc.clone(), port).await
    }
    async fn poll(xhc: Rc<Controller>) -> Result<()> {
        // 4.3 USB Device Initialization
        // USB3: Disconnected -> Polling -> Enabled
        // USB2: Disconnected -> Disabled
        if let Some((port, portsc)) = xhc.portsc_iter().find_map(
            |PortScIteratorItem { port, portsc }| -> Option<(usize, Rc<PortScWrapper>)> {
                let portsc = portsc.upgrade()?;
                if portsc.csc() {
                    portsc.clear_csc();
                    Some((port, portsc))
                } else {
                    None
                }
            },
        ) {
            if portsc.ccs() {
                info!("Port {port}: Device attached: {portsc:?}: ");
                if portsc.state() == PortState::Disabled {
                    match Self::enable_port(xhc.clone(), port).await {
                        Ok(f) => {
                            xhc.device_futures().lock().push_back(f);
                        }
                        Err(e) => {
                            error!(
                                "Failed to initialize an USB2 device on port {}: {:?}",
                                port, e
                            );
                        }
                    }
                } else if portsc.state() == PortState::Enabled {
                    warn!("USB3 is not supported yet (Skipping)");
                } else {
                    error!("Unexpected state");
                }
            } else {
                info!("Port {}: Device detached: {:?}", port, portsc);
            }
        }
        while let Some(e) = xhc.primary_event_ring().lock().pop()? {
            info!("xhci/event: {e:?}");
        }
        let waker = dummy_waker();
        let mut ctx = Context::from_waker(&waker);
        let mut device_futures = xhc.device_futures().lock();
        let mut c = device_futures.cursor_front_mut();
        while let Some(f) = c.current() {
            let r = Future::poll(f.as_mut(), &mut ctx);
            if r.is_ready() {
                c.remove_current();
            } else {
                c.move_next();
            }
        }
        Ok(())
    }
    fn spawn(bdf: BusDeviceFunction) -> Result<Self> {
        spawn_global(async move {
            info!("Initializing the xHC");
            let xhc = create_host_controller(bdf)?;
            let xhc = Rc::new(xhc);
            {
                let xhc = xhc.clone();
                spawn_global(async move {
                    loop {
                        xhc.primary_event_ring().lock().poll().await?;
                        yield_execution().await;
                    }
                })
            }
            info!("Checking if the ring works");
            Self::ensure_ring_is_working(xhc.clone()).await?;
            info!("Entering the main loop");
            loop {
                if let Err(e) = Self::poll(xhc.clone()).await {
                    break Err(e);
                } else {
                    yield_execution().await;
                }
            }
        });
        Ok(Self::default())
    }
}
impl PciDeviceDriverInstance for XhciDriverForPci {
    fn name(&self) -> &str {
        "XhciDriverInstance"
    }
}
impl PciDeviceDriver for XhciDriverForPci {
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
        Ok(Box::new(Self::spawn(bdf)?) as Box<dyn PciDeviceDriverInstance>)
    }
    fn name(&self) -> &str {
        "XhciDriver"
    }
}
