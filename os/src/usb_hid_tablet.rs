extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::input::InputManager;
use crate::input::MouseButtonState;
use crate::memory::Mmio;
use crate::println;
use crate::usb::ConfigDescriptor;
use crate::usb::EndpointDescriptor;
use crate::usb::InterfaceDescriptor;
use crate::usb::UsbDescriptor;
use crate::xhci::context::EndpointContext;
use crate::xhci::context::InputContext;
use crate::xhci::context::InputControlContext;
use crate::xhci::device::UsbDeviceDriverContext;
use crate::xhci::future::TransferEventFuture;
use crate::xhci::ring::CommandRing;
use crate::xhci::ring::TransferRing;
use crate::xhci::trb::GenericTrbEntry;
use crate::xhci::EndpointType;
use crate::xhci::Xhci;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cmp::max;
use core::pin::Pin;

pub async fn init_usb_hid_tablet(
    xhci: &Xhci,
    port: usize,
    slot: u8,
    input_context: &mut Pin<&mut InputContext>,
    ctrl_ep_ring: &mut CommandRing,
    descriptors: &Vec<UsbDescriptor>,
) -> Result<[Option<TransferRing>; 32]> {
    let mut last_config: Option<ConfigDescriptor> = None;
    let mut boot_keyboard_interface: Option<InterfaceDescriptor> = None;
    let mut ep_desc_list: Vec<EndpointDescriptor> = Vec::new();
    for d in descriptors {
        match d {
            UsbDescriptor::Config(e) => {
                if boot_keyboard_interface.is_some() {
                    break;
                }
                last_config = Some(*e);
                ep_desc_list.clear();
            }
            UsbDescriptor::Interface(e) => {
                if let (3, 0, 0) = e.triple() {
                    boot_keyboard_interface = Some(*e)
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
        boot_keyboard_interface.ok_or(Error::Failed("No USB KBD Boot interface found"))?;

    let portsc = xhci.portsc(port)?.upgrade().ok_or("PORTSC was invalid")?;
    let mut input_ctrl_ctx = InputControlContext::default();
    input_ctrl_ctx.add_context(0)?;
    const EP_RING_NONE: Option<TransferRing> = None;
    let mut ep_rings = [EP_RING_NONE; 32];
    let mut last_dci = 1;
    for ep_desc in ep_desc_list {
        match EndpointType::from(&ep_desc) {
            EndpointType::InterruptIn => {
                let tring = TransferRing::new(8)?;
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
            _ => {
                println!("Ignoring {:?}", ep_desc);
            }
        }
    }
    input_context.set_last_valid_dci(last_dci)?;
    input_context.set_input_ctrl_ctx(input_ctrl_ctx)?;
    let cmd = GenericTrbEntry::cmd_configure_endpoint(input_context.as_ref(), slot);
    xhci.send_command(cmd).await?.completed()?;
    xhci.request_set_config(slot, ctrl_ep_ring, config_desc.config_value())
        .await?;
    xhci.request_set_interface(
        slot,
        ctrl_ep_ring,
        interface_desc.interface_number(),
        interface_desc.alt_setting(),
    )
    .await?;
    xhci.request_set_protocol(
        slot,
        ctrl_ep_ring,
        interface_desc.interface_number(),
        0, /* Boot Protocol */
    )
    .await?;
    // 4.6.6 Configure Endpoint
    // When configuring or deconfiguring a device, only after completing a successful
    // Configure Endpoint Command and a successful USB SET_CONFIGURATION
    // request may software schedule data transfers through a newly enabled endpoint
    // or Stream Transfer Ring of the Device Slot.
    for (dci, tring) in ep_rings.iter_mut().enumerate() {
        match tring {
            Some(tring) => {
                tring.fill_ring()?;
                xhci.notify_ep(slot, dci)?;
            }
            None => {}
        }
    }
    Ok(ep_rings)
}

pub async fn attach_usb_device(
    xhci: Rc<Xhci>,
    ddc: UsbDeviceDriverContext,
    mut input_context: Pin<Box<InputContext>>,
    mut ctrl_ep_ring: Pin<Box<CommandRing>>,
) -> Result<()> {
    let port = ddc.port();
    let slot = ddc.slot();
    let descriptors = ddc.descriptors();
    let mut ep_rings = init_usb_hid_tablet(
        &xhci,
        port,
        slot,
        &mut input_context.as_mut(),
        &mut ctrl_ep_ring,
        descriptors,
    )
    .await?;

    let portsc = xhci.portsc(port)?.upgrade().ok_or("PORTSC was invalid")?;
    loop {
        let event_trb = TransferEventFuture::new_on_slot(xhci.primary_event_ring(), slot).await;
        match event_trb {
            Ok(Some(trb)) => {
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
                if let Some(ref mut tring) = ep_rings[trb.dci()] {
                    tring.dequeue_trb(transfer_trb_ptr)?;
                    xhci.notify_ep(slot, trb.dci())?;
                }

                let b = report[0];
                let l = b & 1 != 0;
                let r = b & 2 != 0;
                let c = b & 4 != 0;
                let b = MouseButtonState { l, c, r };

                // 0~32767, top left origin (on QEMU)
                let px = [report[1], report[2]];
                let py = [report[3], report[4]];
                let px = u16::from_le_bytes(px);
                let py = u16::from_le_bytes(py);
                let px = px as f32 / 32768f32;
                let py = py as f32 / 32768f32;
                InputManager::take().push_cursor_input_absolute(px, py, b);
            }
            Ok(None) => {
                // Timed out. Do nothing.
            }
            Err(e) => {
                println!("e: {:?}", e);
            }
        }
        if !portsc.ccs() {
            return Err(Error::FailedString(format!("port {} disconnected", port)));
        }
    }
}
