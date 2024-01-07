extern crate alloc;

use crate::error::Result;
use crate::executor::yield_execution;
use crate::executor::Task;
use crate::executor::ROOT_EXECUTOR;
use crate::pci::BusDeviceFunction;
use crate::pci::PciDeviceDriver;
use crate::pci::PciDeviceDriverInstance;
use crate::pci::VendorDeviceId;
use crate::xhci::Xhci;
use alloc::boxed::Box;
use alloc::rc::Rc;

#[derive(Default)]
pub struct XhciDriverForPci {}
impl XhciDriverForPci {
    fn spawn(bdf: BusDeviceFunction) -> Result<Self> {
        (*ROOT_EXECUTOR.lock()).spawn(Task::new(async move {
            let mut xhc = Xhci::new(bdf)?;
            xhc.ensure_ring_is_working().await?;
            let xhc = Rc::new(xhc);
            loop {
                if let Err(e) = xhc.as_ref().poll(xhc.clone()).await {
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
