extern crate alloc;

use crate::info;
use crate::print::hexdump;
use crate::result::Result;
use crate::usb::*;
use crate::xhci::CommandRing;
use crate::xhci::Controller;
use alloc::rc::Rc;
use alloc::vec::Vec;

pub async fn start_usb_tablet(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut CommandRing,
    device_descriptor: &UsbDeviceDescriptor,
    descriptors: &Vec<UsbDescriptor>,
) -> Result<()> {
    // vid:pid = 0x0627:0x0001
    if device_descriptor.device_class != 0
        || device_descriptor.device_subclass != 0
        || device_descriptor.device_protocol != 0
        || device_descriptor.vendor_id != 0x0627
        || device_descriptor.product_id != 0x0001
    {
        return Err("Not a USB Tablet");
    }
    let (_config_desc, interface_desc, _) = pick_interface_with_triple(descriptors, (3, 0, 0))
        .ok_or("No USB KBD Boot interface found")?;
    info!("USB tablet found");
    let report =
        request_hid_report_descriptor(xhc, slot, ctrl_ep_ring, interface_desc.interface_number)
            .await?;
    info!("Report Descriptor:");
    hexdump(&report);
    Ok(())
}
