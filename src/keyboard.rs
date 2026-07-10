extern crate alloc;

use crate::cui::Console;
use crate::executor::spawn_global;
use crate::info;
use crate::result::Result;
use crate::usb;
use crate::usb::request_get_configuration;
use crate::usb::request_get_interface;
use crate::usb::request_get_protocol;
use crate::usb::UsbDescriptor;
use crate::usb::UsbDeviceDescriptor;
use crate::usb::UsbHidProtocol;
use crate::xhci::Controller;
use crate::xhci::TransferRing;
use alloc::collections::BTreeSet;
use alloc::rc::Rc;
use alloc::vec::Vec;

#[derive(Debug, PartialEq, Eq)]
pub enum KeyEvent {
    None,
    Char(char),
    Unknown(u8),
    CursorDown,
    CursorLeft,
    CursorRight,
    CursorUp,
    CtrlLeft,
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
            79 => KeyEvent::CursorRight,
            80 => KeyEvent::CursorLeft,
            81 => KeyEvent::CursorDown,
            82 => KeyEvent::CursorUp,
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

pub struct UsbKeyboardDriverState {
    xhc: Rc<Controller>,
    slot: u8,
    ctrl_ep_ring: TransferRing,
    #[allow(unused)]
    interface_number: u8,
}
impl UsbKeyboardDriverState {
    async fn setup(
        xhc: Rc<Controller>,
        _port: usize,
        slot: u8,
        mut ctrl_ep_ring: TransferRing,
        descriptors: Vec<UsbDescriptor>,
    ) -> Result<Self> {
        for d in &descriptors {
            if let UsbDescriptor::Config(e) = d {
                info!("C: {e:?}")
            } else if let UsbDescriptor::Interface(e) = d {
                info!("I:   {e:?}")
            } else if let UsbDescriptor::Endpoint(e) = d {
                info!("E:     {e:?}")
            } else if let UsbDescriptor::Unknown {
                desc_len: _,
                desc_type: 0x30,
                payload,
            } = d
            {
                info!("SSEC:    {payload:?}")
            }
        }

        let (config_desc, interface_desc, _descriptors_in_interface) =
            usb::pick_interface_with_triple(
                &descriptors,
                usb::TRIPLE_FOR_HID_BOOT_KBD,
            )
            .ok_or("No USB KBD Boot interface found")?;

        let config_now =
            request_get_configuration(&xhc, slot, &mut ctrl_ep_ring, 1).await?;
        info!("config_now = {config_now}");
        if config_now != config_desc.config_value() {
            xhc.request_set_config(
                slot,
                &mut ctrl_ep_ring,
                config_desc.config_value(),
            )
            .await?;
        }

        let alt_int_now = request_get_interface(
            &xhc,
            slot,
            &mut ctrl_ep_ring,
            1,
            interface_desc.interface_number,
        )
        .await?;
        info!(
            "alt_int_now({}) = {alt_int_now}",
            interface_desc.interface_number
        );
        if alt_int_now != interface_desc.alt_setting {
            xhc.request_set_interface(
                slot,
                &mut ctrl_ep_ring,
                interface_desc.interface_number,
                interface_desc.alt_setting,
            )
            .await?;
        }

        let prot_now = request_get_protocol(
            &xhc,
            slot,
            &mut ctrl_ep_ring,
            1,
            interface_desc.interface_number,
        )
        .await?;
        info!("prot_now = {prot_now}");
        if prot_now != usb::UsbHidProtocol::BootProtocol as u8 {
            xhc.request_set_protocol(
                slot,
                &mut ctrl_ep_ring,
                interface_desc.interface_number,
                usb::UsbHidProtocol::BootProtocol as u8,
            )
            .await?;
            let prot_now = request_get_protocol(
                &xhc,
                slot,
                &mut ctrl_ep_ring,
                1,
                interface_desc.interface_number,
            )
            .await?;
            info!("prot_now = {prot_now}");
            if prot_now != UsbHidProtocol::BootProtocol as u8 {
                info!("prot_now is not Boot Protocol(0)!!!");
                return Err("prot_now is not Boot Protocol");
            }
        }

        Ok(Self {
            xhc,
            slot,
            ctrl_ep_ring,
            interface_number: interface_desc.interface_number,
        })
    }
    async fn poll_input(&mut self) -> Result<()> {
        let mut console = Console::default();
        let mut prev_pressed = BTreeSet::new();
        loop {
            let pressed = {
                let report = usb::request_hid_report(
                    &self.xhc,
                    self.slot,
                    &mut self.ctrl_ep_ring,
                )
                .await?;
                BTreeSet::from_iter(
                    report.into_iter().skip(2).filter(|id| *id != 0),
                )
            };
            let diff = pressed.symmetric_difference(&prev_pressed);
            for id in diff {
                let e = KeyEvent::from_usb_key_id(*id);
                if pressed.contains(id) {
                    console.handle_key_down(e);
                }
            }
            prev_pressed = pressed;
        }
    }
}
pub struct UsbKeyboardDriver;
impl usb::UsbDeviceDriver for UsbKeyboardDriver {
    fn is_compatible(
        &self,
        descriptors: &[UsbDescriptor],
        _device_descriptor: &UsbDeviceDescriptor,
    ) -> bool {
        usb::pick_interface_with_triple(descriptors, (3, 1, 1)).is_some()
    }
    fn start(
        &self,
        xhc: Rc<Controller>,
        port: usize,
        slot: u8,
        ctrl_ep_ring: TransferRing,
        descriptors: Vec<UsbDescriptor>,
        _device_descriptor: &UsbDeviceDescriptor,
    ) {
        spawn_global(async move {
            let mut ds = UsbKeyboardDriverState::setup(
                xhc,
                port,
                slot,
                ctrl_ep_ring,
                descriptors,
            )
            .await?;
            ds.poll_input().await
        });
    }
}
