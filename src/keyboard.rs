extern crate alloc;

use crate::info;
use crate::result::Result;
use crate::usb::*;
use crate::xhci::CommandRing;
use crate::xhci::Controller;
use alloc::collections::BTreeSet;
use alloc::rc::Rc;
use alloc::vec::Vec;

#[derive(Debug, PartialEq, Eq)]
pub enum KeyEvent {
    None,
    Char(char),
    Unknown(u8),
    Enter,
}
impl KeyEvent {
    pub fn from_usb_key_id(usage_id: u8) -> Self {
        match usage_id {
            0 => KeyEvent::None,
            4..=29 => KeyEvent::Char((b'a' + usage_id - 4) as char),
            30..=39 => KeyEvent::Char((b'0' + (usage_id + 1) % 10) as char),
            40 => KeyEvent::Enter,
            42 => KeyEvent::Char(0x08 as char),
            44 => KeyEvent::Char(' '),
            45 => KeyEvent::Char('-'),
            51 => KeyEvent::Char(':'),
            54 => KeyEvent::Char(','),
            55 => KeyEvent::Char('.'),
            56 => KeyEvent::Char('/'),
            _ => KeyEvent::Unknown(usage_id),
        }
    }
    pub fn to_char(&self) -> Option<char> {
        match self {
            KeyEvent::Char(c) => Some(*c),
            KeyEvent::Enter => Some('\n'),
            _ => None,
        }
    }
}

pub async fn start_usb_keyboard(
    xhc: &Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: &mut CommandRing,
    descriptors: &Vec<UsbDescriptor>,
) -> Result<()> {
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
                if let (3, 1, 1) = e.triple() {
                    boot_keyboard_interface = Some(*e)
                }
            }
            UsbDescriptor::Endpoint(e) => {
                ep_desc_list.push(*e);
            }
            _ => {}
        }
    }
    let config_desc = last_config.ok_or("No USB KBD Boot config found")?;
    let interface_desc = boot_keyboard_interface.ok_or("No USB KBD Boot interface found")?;
    xhc.request_set_config(slot, ctrl_ep_ring, config_desc.config_value())
        .await?;
    xhc.request_set_interface(
        slot,
        ctrl_ep_ring,
        interface_desc.interface_number,
        interface_desc.alt_setting,
    )
    .await?;
    xhc.request_set_protocol(
        slot,
        ctrl_ep_ring,
        interface_desc.interface_number,
        UsbHidProtocol::BootProtocol as u8,
    )
    .await?;
    let mut prev_pressed = BTreeSet::new();
    loop {
        let pressed = {
            let report = request_hid_report(xhc, slot, ctrl_ep_ring).await?;
            BTreeSet::from_iter(report.into_iter().skip(2).filter(|id| *id != 0))
        };
        let diff = pressed.symmetric_difference(&prev_pressed);
        for id in diff {
            let e = KeyEvent::from_usb_key_id(*id);
            if pressed.contains(id) {
                info!("usb_keyboard: key down: {id} = {e:?}");
            } else {
                info!("usb_keyboard: key up  : {id} = {e:?}");
            }
        }
        prev_pressed = pressed;
    }
}
