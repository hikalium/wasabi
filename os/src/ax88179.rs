extern crate alloc;

use crate::arch::x86_64::paging::Mmio;
use crate::error::Error;
use crate::error::Result;
use crate::executor::delay;
use crate::println;
use crate::usb::DeviceDescriptor;
use crate::usb::UsbDescriptor;
use crate::xhci::context::InputContext;
use crate::xhci::future::TransferEventFuture;
use crate::xhci::ring::CommandRing;
use crate::xhci::trb::DataStageTrb;
use crate::xhci::trb::SetupStageTrb;
use crate::xhci::trb::StatusStageTrb;
use crate::xhci::Xhci;
use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;
use core::pin::Pin;

// https://github.com/lwhsu/if_axge-kmod/blob/be7510b1bc7e5974963fe17e67462d3689e80631/if_axgereg.h#L36
const REQUEST_ACCESS_MAC: u8 = 0x01;

// https://github.com/lwhsu/if_axge-kmod/blob/be7510b1bc7e5974963fe17e67462d3689e80631/if_axgereg.h#L73
const VALUE_NODE_ID: u16 = 0x0010;

// https://github.com/lwhsu/if_axge-kmod/blob/be7510b1bc7e5974963fe17e67462d3689e80631/if_axgereg.h#L100
// AXGE_PHYPWR_RSTCTL
const MAC_REG_PHYPWR_RSTCTL: u16 = 0x0026;
// AXGE_PHYPWR_RSTCTL_IPRL
const MAC_REG_PHYPWR_RSTCTL_IPRL: u16 = 0x0020;

const MAC_REG_CLK_SELECT: u16 = 0x0033;
const MAC_REG_CLK_SELECT_BCS: u8 = 0x01;
const MAC_REG_CLK_SELECT_ACS: u8 = 0x02;

async fn read_from_device<T: Sized>(
    xhci: &mut Xhci,
    slot: u8,
    ctrl_ep_ring: &mut CommandRing,
    request: u8,
    value: u16,
    index: u16,
    buf: Pin<&mut [T]>,
) -> Result<()> {
    // https://github.com/lwhsu/if_axge-kmod/blob/be7510b1bc7e5974963fe17e67462d3689e80631/if_axge.c#L201
    ctrl_ep_ring.push(
        SetupStageTrb::new_vendor_device_in(request, value, index, buf.len() as u16).into(),
    )?;
    let trb_ptr_waiting = ctrl_ep_ring.push(DataStageTrb::new_in(buf).into())?;
    ctrl_ep_ring.push(StatusStageTrb::new_out().into())?;
    xhci.notify_ep(slot, 1);
    TransferEventFuture::new_with_timeout(xhci.primary_event_ring(), trb_ptr_waiting, 10 * 1000)
        .await?
        .ok_or(Error::Failed("Timed out"))?
        .completed()
}

async fn write_to_device<T: Sized>(
    xhci: &mut Xhci,
    slot: u8,
    ctrl_ep_ring: &mut CommandRing,
    request: u8,
    index: u16,
    reg: u16,
    buf: Pin<&mut [T]>,
) -> Result<()> {
    ctrl_ep_ring
        .push(SetupStageTrb::new_vendor_device_out(request, reg, index, buf.len() as u16).into())?;
    let trb_ptr_waiting = ctrl_ep_ring.push(DataStageTrb::new_out(buf).into())?;
    ctrl_ep_ring.push(StatusStageTrb::new_in().into())?;
    xhci.notify_ep(slot, 1);
    TransferEventFuture::new_with_timeout(xhci.primary_event_ring(), trb_ptr_waiting, 10 * 1000)
        .await?
        .ok_or(Error::Failed("Timed out"))?
        .completed()
}

async fn write_mac_reg_u16(
    xhci: &mut Xhci,
    slot: u8,
    ctrl_ep_ring: &mut CommandRing,
    request: u8,
    index: u16,
    value: u16,
) -> Result<()> {
    let mut data = Box::pin([value as u8, (value >> 8) as u8]);
    write_to_device(xhci, slot, ctrl_ep_ring, request, 2, index, data.as_mut()).await?;
    Ok(())
}

async fn write_mac_reg_u8(
    xhci: &mut Xhci,
    slot: u8,
    ctrl_ep_ring: &mut CommandRing,
    request: u8,
    index: u16,
    value: u8,
) -> Result<()> {
    let mut data = Box::pin([value]);
    write_to_device(xhci, slot, ctrl_ep_ring, request, 1, index, data.as_mut()).await?;
    Ok(())
}

async fn request_read_mac_addr(
    xhci: &mut Xhci,
    slot: u8,
    ctrl_ep_ring: &mut CommandRing,
) -> Result<[u8; 6]> {
    // https://github.com/lwhsu/if_axge-kmod/blob/8afe945e769c87f2eaaf62468e30fe860108d26f/if_axge.c#L414
    let mac = [0u8; 6];
    let mut mac = Box::pin(mac);
    read_from_device(
        xhci,
        slot,
        ctrl_ep_ring,
        REQUEST_ACCESS_MAC,
        VALUE_NODE_ID,
        6,
        mac.as_mut(),
    )
    .await?;
    Ok(*mac)
}
pub async fn attach_usb_device(
    xhci: &mut Xhci,
    port: usize,
    slot: u8,
    input_context: &mut Pin<&mut InputContext>,
    ctrl_ep_ring: &mut CommandRing,
    device_descriptor: &DeviceDescriptor,
    descriptors: &Vec<UsbDescriptor>,
) -> Result<()> {
    xhci.request_set_config(slot, ctrl_ep_ring, 1).await?;
    // Power up the PHY
    // https://github.com/lwhsu/if_axge-kmod/blob/be7510b1bc7e5974963fe17e67462d3689e80631/if_axge.c#L367
    write_mac_reg_u16(
        xhci,
        slot,
        ctrl_ep_ring,
        REQUEST_ACCESS_MAC,
        MAC_REG_PHYPWR_RSTCTL,
        0,
    )
    .await?;
    write_mac_reg_u16(
        xhci,
        slot,
        ctrl_ep_ring,
        REQUEST_ACCESS_MAC,
        MAC_REG_PHYPWR_RSTCTL,
        MAC_REG_PHYPWR_RSTCTL_IPRL,
    )
    .await?;
    delay().await;
    write_mac_reg_u8(
        xhci,
        slot,
        ctrl_ep_ring,
        REQUEST_ACCESS_MAC,
        MAC_REG_CLK_SELECT,
        MAC_REG_CLK_SELECT_ACS | MAC_REG_CLK_SELECT_BCS,
    )
    .await?;

    // Read MAC Address
    let mac = request_read_mac_addr(xhci, slot, ctrl_ep_ring).await?;
    println!(
        "MAC Addr: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        mac.as_ref()[0],
        mac.as_ref()[1],
        mac.as_ref()[2],
        mac.as_ref()[3],
        mac.as_ref()[4],
        mac.as_ref()[5],
    );
    let mut ep_desc_list = Vec::new();
    for d in descriptors {
        if let UsbDescriptor::Endpoint(e) = d {
            ep_desc_list.push(*e);
        }
    }
    let mut ep_rings = xhci
        .setup_endpoints(port, slot, input_context, &ep_desc_list)
        .await?;
    let portsc = xhci.portsc(port)?;

    loop {
        let event_trb = TransferEventFuture::new_on_slot(xhci.primary_event_ring(), slot).await;
        match event_trb {
            Ok(Some(trb)) => {
                let transfer_trb_ptr = trb.data() as usize;
                let report = unsafe {
                    Mmio::<[u8; 4096]>::from_raw(
                        *(transfer_trb_ptr as *const usize) as *mut [u8; 4096],
                    )
                };
                println!(
                    "slot = {}, dci = {}, length = {}",
                    slot,
                    trb.dci(),
                    trb.transfer_length()
                );
                // hexdump(&report.as_ref()[0..trb.transfer_length()]);
                if let Some(ref mut tring) = ep_rings[trb.dci()] {
                    tring.dequeue_trb(transfer_trb_ptr)?;
                    xhci.notify_ep(slot, trb.dci());
                }
            }
            Ok(None) => {
                // Timed out. Do nothing.
            }
            Err(e) => {
                println!("Error: {:?}", e);
            }
        }
        if !portsc.ccs() {
            return Err(Error::FailedString(format!("port {} disconnected", port)));
        }
    }
}
