extern crate alloc;

use crate::executor::spawn_global;
use crate::info;
use crate::result::Result;
use crate::usb::descriptors_under_config;
use crate::usb::pick_interface_with_triple;
use crate::usb::UsbDescriptor;
use crate::usb::UsbDeviceDescriptor;
use crate::usb::UsbDeviceDriver;
use crate::xhci::Controller;
use crate::xhci::TransferRing;
use alloc::rc::Rc;
use alloc::vec::Vec;

pub struct UsbNcmDriver;
impl UsbNcmDriver {
    async fn run(
        _xhc: &Rc<Controller>,
        _port: usize,
        _slot: u8,
        _ctrl_ep_ring: &mut TransferRing,
        descriptors: &[UsbDescriptor],
    ) -> Result<()> {
        /*
        interface 0 alt 0 02:0D:00
                02: Communication Interface Class
                0D: Network Control Model Subclass
                00: Protocol defined in the USB Spec
            EP 1 interrupt in mps 16 interval 11
                SSEC 0, 0, 8, 0
        interface 1 alt 0 0A:00:01
        interface 1 alt 1 0A:00:01
            EP 2 bulk in mps 0x400 interval 0
                SSEC 5, 0, 0, 0
            EP 3 bulk out mps 0x400 interval 0
                SSEC 5, 0, 0, 0
        */
        let (config_desc, _, _) =
            pick_interface_with_triple(descriptors, (2, 13, 0))
                .ok_or("No USB NCM Communications interface found")?;
        info!("C: {config_desc:?}");
        let desc_under_config =
            descriptors_under_config(descriptors, config_desc.config_value());
        for d in &desc_under_config {
            if let UsbDescriptor::Interface(e) = d {
                info!("I:   {e:?}")
            } else if let UsbDescriptor::Endpoint(e) = d {
                info!("E:     {e:?}")
            } else if let UsbDescriptor::Unknown {
                desc_type: 0x30,
                payload,
                ..
            } = d
            {
                info!("SSEC:    {payload:?}")
            } else if let UsbDescriptor::Unknown { .. } = d {
                info!("?   :    {d:?}")
            }
        }
        Ok(())
    }
}
impl UsbDeviceDriver for UsbNcmDriver {
    fn is_compatible(
        &self,
        descriptors: &[UsbDescriptor],
        _device_descriptor: &UsbDeviceDescriptor,
    ) -> bool {
        // Note: QEMU's usb-nic does not have this interface.
        pick_interface_with_triple(
            descriptors,
            (
                2,  /* Communications Interface Class [cdc_1_2] 4.2 */
                13, /* Network Control Model [cdc_1_2] 4.3 */
                0,
            ),
        )
        .is_some()
    }
    fn start(
        &self,
        xhc: Rc<Controller>,
        port: usize,
        slot: u8,
        mut ctrl_ep_ring: TransferRing,
        descriptors: Vec<UsbDescriptor>,
        _device_descriptor: &UsbDeviceDescriptor,
    ) {
        spawn_global(async move {
            Self::run(&xhc, port, slot, &mut ctrl_ep_ring, &descriptors).await
        });
    }
}
