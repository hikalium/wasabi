extern crate alloc;

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
use crate::xhci::registers::PortScIteratorItem;
use crate::xhci::registers::PortScWrapper;
use crate::xhci::registers::PortState;
use crate::xhci::ring::TrbRing;
use crate::xhci::trb::GenericTrbEntry;
use crate::xhci::Xhci;
use alloc::boxed::Box;
use alloc::rc::Rc;
use core::future::Future;
use core::task::Context;

#[derive(Default)]
pub struct XhciDriverForPci {}
impl XhciDriverForPci {
    async fn ensure_ring_is_working(xhc: Rc<Xhci>) -> Result<()> {
        for _ in 0..TrbRing::NUM_TRB * 2 + 1 {
            xhc.send_command(GenericTrbEntry::cmd_no_op())
                .await?
                .completed()?;
        }
        Ok(())
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
                    match xhc.enable_port(xhc.clone(), port).await {
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
