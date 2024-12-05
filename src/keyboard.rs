extern crate alloc;

use crate::executor::spawn_global;
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

pub struct UsbKeyboardDriver;
impl UsbKeyboardDriver {
    async fn run(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
        descriptors: &[UsbDescriptor],
    ) -> Result<()> {
        let (config_desc, interface_desc, _) = pick_interface_with_triple(descriptors, (3, 1, 1))
            .ok_or("No USB KBD Boot interface found")?;
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
}
impl UsbDeviceDriver for UsbKeyboardDriver {
    fn is_compatible(
        descriptors: &[UsbDescriptor],
        _device_descriptor: &UsbDeviceDescriptor,
    ) -> bool {
        pick_interface_with_triple(descriptors, (3, 1, 1)).is_some()
    }
    fn start(
        xhc: Rc<Controller>,
        slot: u8,
        mut ctrl_ep_ring: CommandRing,
        descriptors: Vec<UsbDescriptor>,
    ) {
        spawn_global(async move { Self::run(&xhc, slot, &mut ctrl_ep_ring, &descriptors).await });
    }
}
