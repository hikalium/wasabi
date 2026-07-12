extern crate alloc;

use crate::executor::spawn_global;
use crate::executor::with_timeout;
use crate::info;
use crate::result::Result;
use crate::usb;
use crate::usb::descriptors_under_config;
use crate::usb::pick_interface_with_triple;
use crate::usb::UsbDescriptor;
use crate::usb::UsbDeviceDescriptor;
use crate::usb::UsbDeviceDriver;
use crate::xhci::Controller;
use crate::xhci::TransferRing;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::time::Duration;

pub struct UsbNcmDriver;
impl UsbNcmDriver {
    pub async fn request_get_net_address(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
    ) -> Result<Vec<u8>> {
        let buf = vec![0u8; 6];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        xhc.request_transfer_from_class_interface(
            slot,
            ctrl_ep_ring,
            0x81,
            0,
            0,
            &mut buf,
        )
        .await?;
        Ok(buf.to_vec())
    }
    pub async fn request_get_ntb_parameters(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
    ) -> Result<Vec<u8>> {
        let buf = vec![0u8; 28];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        xhc.request_transfer_from_class_interface(
            slot,
            ctrl_ep_ring,
            0x80,
            0,
            0,
            &mut buf,
        )
        .await?;
        Ok(buf.to_vec())
    }
    pub async fn request_get_network_connection(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
    ) -> Result<Vec<u8>> {
        let buf = vec![0u8; 6];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        xhc.request_transfer_from_class_interface(
            slot,
            ctrl_ep_ring,
            0x81,
            0,
            0,
            &mut buf,
        )
        .await?;
        Ok(buf.to_vec())
    }
    async fn run(
        xhc: &Rc<Controller>,
        _port: usize,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
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
        let mut mac_addr_index = 0;
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
            } else if let UsbDescriptor::Unknown {
                desc_type: 0x24, /* CS_INTERFACE [ncm_1_1] Table 6-2 */
                payload,
                ..
            } = d
            {
                let subtype = payload.first().cloned().unwrap_or_default();
                match subtype {
                    0x0F => {
                        /* Ethernet Networking Functional Descriptor [cdc_1_2
                         * Table 13] */
                        // Expected to be non-zero.
                        mac_addr_index =
                            payload.get(1).cloned().unwrap_or_default();
                    }
                    _ => {
                        info!("?   :    {d:?}")
                    }
                }
            } else if let UsbDescriptor::Unknown { .. } = d {
                info!("?   :    {d:?}")
            }
        }

        let mac_addr = {
            let res = with_timeout(
                Duration::from_secs(1),
                usb::request_string_descriptor_zero(xhc, slot, ctrl_ep_ring),
            )
            .await?;
            // If there is one lang_id, bLength will be 4
            if res[0] < 4 {
                return Err("string desc zero too short");
            }
            let lang_id = u16::from_le_bytes([res[2], res[3]]);
            with_timeout(
                Duration::from_secs(1),
                usb::request_string_descriptor(
                    xhc,
                    slot,
                    ctrl_ep_ring,
                    lang_id,
                    mac_addr_index,
                ),
            )
            .await?
        };
        info!("iMacAddress: {mac_addr:?}");

        xhc.request_set_config(slot, ctrl_ep_ring, 2).await?;
        xhc.request_set_interface(slot, ctrl_ep_ring, 0, 0).await?;
        // start operation!
        xhc.request_set_interface(slot, ctrl_ep_ring, 1, 1).await?;

        let ntbparams =
            Self::request_get_ntb_parameters(xhc, slot, ctrl_ep_ring).await?;
        info!("ntbparams: {ntbparams:?}");
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
