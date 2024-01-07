extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::executor::dummy_waker;
use crate::executor::yield_execution;
use crate::executor::Task;
use crate::executor::ROOT_EXECUTOR;
use crate::pci::BusDeviceFunction;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::print;
use crate::println;
use crate::xhci::context::EndpointContext;
use crate::xhci::context::InputContext;
use crate::xhci::context::InputControlContext;
use crate::xhci::context::OutputContext;
use crate::xhci::registers::PortLinkState;
use crate::xhci::registers::PortScIteratorItem;
use crate::xhci::registers::PortScWrapper;
use crate::xhci::registers::PortState;
use crate::xhci::ring::CommandRing;
use crate::xhci::ring::TrbRing;
use crate::xhci::trb::GenericTrbEntry;
use crate::xhci::Xhci;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use core::future::Future;
use core::pin::Pin;
use core::task::Context;

#[derive(Default)]
pub struct XhciDriverForPci {}
impl XhciDriverForPci {
    async fn address_device(
        xhc: Rc<Xhci>,
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
        let portsc = xhc
            .portsc
            .get(port)?
            .upgrade()
            .ok_or("PORTSC was invalid")?;
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
        xhc.device_ready(xhc.clone(), port, slot, input_context, ctrl_ep_ring)
            .await
    }
    async fn ensure_ring_is_working(xhc: Rc<Xhci>) -> Result<()> {
        for _ in 0..TrbRing::NUM_TRB * 2 + 1 {
            xhc.send_command(GenericTrbEntry::cmd_no_op())
                .await?
                .completed()?;
        }
        Ok(())
    }
    async fn enable_slot(
        xhc: Rc<Xhci>,
        port: usize,
    ) -> Result<Pin<Box<dyn Future<Output = Result<()>>>>> {
        let portsc = xhc
            .portsc
            .get(port)?
            .upgrade()
            .ok_or("PORTSC was invalid")?;
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
    async fn enable_port(
        xhc: Rc<Xhci>,
        port: usize,
    ) -> Result<Pin<Box<dyn Future<Output = Result<()>>>>> {
        // Reset port to enable the port (via Reset state)
        xhc.reset_port(port).await?;
        loop {
            let portsc = xhc
                .portsc
                .get(port)?
                .upgrade()
                .ok_or("PORTSC was invalid")?;
            if let (PortState::Enabled, PortLinkState::U0) = (portsc.state(), portsc.pls()) {
                break;
            }
            yield_execution().await;
        }
        Self::enable_slot(xhc.clone(), port).await
    }
    async fn poll(xhc: Rc<Xhci>) -> Result<()> {
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
                print!("Port {port}: Device attached: {portsc:?}: ");
                if portsc.state() == PortState::Disabled {
                    println!("USB2");
                    match Self::enable_port(xhc.clone(), port).await {
                        Ok(f) => {
                            println!("device future attached",);
                            xhc.device_futures.lock().push_back(f);
                        }
                        Err(e) => {
                            println!(
                                "Failed to initialize an USB2 device on port {}: {:?}",
                                port, e
                            );
                        }
                    }
                } else if portsc.state() == PortState::Enabled {
                    println!("USB3 (Skipping)");
                } else {
                    println!("Unexpected state");
                }
            } else {
                println!("Port {}: Device detached: {:?}", port, portsc);
            }
        }
        let waker = dummy_waker();
        let mut ctx = Context::from_waker(&waker);
        let mut device_futures = xhc.device_futures.lock();
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
        (*ROOT_EXECUTOR.lock()).spawn(Task::new(async move {
            let xhc = Xhci::new(bdf)?;
            let xhc = Rc::new(xhc);
            Self::ensure_ring_is_working(xhc.clone()).await?;
            loop {
                if let Err(e) = Self::poll(xhc.clone()).await {
                    break Err(e);
                } else {
                    yield_execution().await;
                }
            }
        }));
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
