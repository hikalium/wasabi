extern crate alloc;

use crate::boot_info::BootInfo;
use crate::error;
use crate::error::Error;
use crate::error::Result;
use crate::input::InputManager;
use crate::memory::Mmio;
use crate::usb::descriptor::ConfigDescriptor;
use crate::usb::descriptor::EndpointDescriptor;
use crate::usb::descriptor::InterfaceDescriptor;
use crate::usb::descriptor::UsbDescriptor;
use crate::xhci::device::UsbDeviceDriverContext;
use crate::xhci::device::UsbHidProtocol;
use crate::xhci::future::EventFuture;
use alloc::format;
use alloc::vec::Vec;
use noli::bitmap::Bitmap;
use sabi::MouseButtonState;
use sabi::MouseEvent;
use sabi::PointerPosition;

pub fn pick_config(
    descriptors: &Vec<UsbDescriptor>,
) -> Result<(
    ConfigDescriptor,
    InterfaceDescriptor,
    Vec<EndpointDescriptor>,
)> {
    let mut last_config: Option<ConfigDescriptor> = None;
    let mut selected_interface: Option<InterfaceDescriptor> = None;
    let mut ep_desc_list: Vec<EndpointDescriptor> = Vec::new();
    for d in descriptors {
        match d {
            UsbDescriptor::Config(e) => {
                if selected_interface.is_some() {
                    break;
                }
                last_config = Some(*e);
                ep_desc_list.clear();
            }
            UsbDescriptor::Interface(e) => {
                if let (3, 0, 0) = e.triple() {
                    selected_interface = Some(*e)
                }
            }
            UsbDescriptor::Endpoint(e) => {
                ep_desc_list.push(*e);
            }
            _ => {}
        }
    }
    let config_desc = last_config.ok_or(Error::Failed("No USB KBD Boot config found"))?;
    let interface_desc =
        selected_interface.ok_or(Error::Failed("No USB KBD Boot interface found"))?;
    Ok((config_desc, interface_desc, ep_desc_list))
}

pub async fn init_usb_hid_tablet(ddc: &mut UsbDeviceDriverContext) -> Result<()> {
    let descriptors = ddc.descriptors();
    let (config_desc, interface_desc, ep_desc_list) = pick_config(descriptors)?;
    ddc.set_config(config_desc.config_value()).await?;
    ddc.set_interface(&interface_desc).await?;
    ddc.set_protocol(&interface_desc, UsbHidProtocol::BootProtocol)
        .await?;
    // 4.6.6 Configure Endpoint
    // When configuring or deconfiguring a device, only after completing a successful
    // Configure Endpoint Command and a successful USB SET_CONFIGURATION
    // request may software schedule data transfers through a newly enabled endpoint
    // or Stream Transfer Ring of the Device Slot.
    for ep_desc in &ep_desc_list {
        let ep_ring = ddc
            .ep_ring(ep_desc.dci())?
            .as_ref()
            .ok_or(Error::Failed("Endpoint not created"))?;
        ep_ring.fill_ring()?;
        ddc.notify_ep(ep_desc)?;
    }
    Ok(())
}

pub async fn attach_usb_device(mut ddc: UsbDeviceDriverContext) -> Result<()> {
    init_usb_hid_tablet(&mut ddc).await?;

    let port = ddc.port();
    let slot = ddc.slot();
    let xhci = ddc.xhci();
    let portsc = xhci.portsc(port)?.upgrade().ok_or("PORTSC was invalid")?;

    let vram = BootInfo::take().vram();
    let w = vram.width() as f64;
    let h = vram.height() as f64;
    let max_x = w - 1.0;
    let max_y = h - 1.0;

    let event_trb = EventFuture::new_transfer_event_on_slot(xhci.primary_event_ring(), slot);
    loop {
        let event_trb = event_trb.clone().await;
        match event_trb {
            Ok(trb) => {
                let transfer_trb_ptr = trb.data() as usize;
                let mut report = [0u8; 8];
                report.copy_from_slice(
                    unsafe {
                        Mmio::<[u8; 8]>::from_raw(
                            *(transfer_trb_ptr as *const usize) as *mut [u8; 8],
                        )
                    }
                    .as_ref(),
                );
                if let Some(ref mut tring) = ddc.ep_ring(trb.dci())?.as_ref() {
                    tring.dequeue_trb(transfer_trb_ptr)?;
                    xhci.notify_ep(slot, trb.dci())?;
                }

                let b = report[0];
                let l = b & 1 != 0;
                let r = b & 2 != 0;
                let c = b & 4 != 0;
                let button = MouseButtonState::from_lcr(l, c, r);

                // 0~32767, top left origin (on QEMU)
                let px = [report[1], report[2]];
                let py = [report[3], report[4]];
                let px = u16::from_le_bytes(px);
                let py = u16::from_le_bytes(py);
                let px = px as f64 / 32768f64;
                let py = py as f64 / 32768f64;
                // convert to the screen corrdinates
                let px = px * w;
                let py = py * h;
                let px = unsafe { px.clamp(0.0, max_x).to_int_unchecked() };
                let py = unsafe { py.clamp(0.0, max_y).to_int_unchecked() };
                let position = PointerPosition::from_xy(px, py);

                InputManager::take().push_cursor_input_absolute(MouseEvent { button, position });
            }
            Err(e) => {
                error!("e: {:?}", e);
            }
        }
        if !portsc.ccs() {
            return Err(Error::FailedString(format!("port {} disconnected", port)));
        }
    }
}
