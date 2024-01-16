extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::println;
use crate::usb::descriptor::EndpointDescriptor;
use crate::usb::descriptor::InterfaceDescriptor;
use crate::usb::descriptor::UsbDescriptor;
use crate::xhci::context::InputContext;
use crate::xhci::controller::Controller;
use crate::xhci::future::TransferEventFuture;
use crate::xhci::ring::CommandRing;
use crate::xhci::ring::TransferRing;
use crate::xhci::trb::GenericTrbEntry;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::pin::Pin;

#[derive(Debug, Clone)]
pub struct Event {
    data: [u8; 8],
}
impl Event {
    pub fn data(&self) -> [u8; 8] {
        self.data
    }
}

#[derive(Default)]
pub struct EventQueue {
    queue: VecDeque<Event>,
}
impl EventQueue {
    pub fn push(&mut self, data: Event) {
        if self.queue.len() > 16 {
            let discarded = self.queue.pop_front();
            println!("EventQueue: Warning: No more capacity. Discarded: {discarded:?}");
        }
        self.queue.push_back(data);
    }
    pub fn pop(&mut self) -> Option<Event> {
        self.queue.pop_front()
    }
}

// [hid_1_11]:
// 7.2.5 Get_Protocol Request
// 7.2.6 Set_Protocol Request
#[repr(u8)]
pub enum UsbHidProtocol {
    BootProtocol = 0,
    ReportProtocol = 1,
}

pub struct UsbDeviceDriverContext {
    port: usize,
    slot: u8,
    xhci: Rc<Controller>,
    descriptors: Vec<UsbDescriptor>,
    _input_context: Pin<Box<InputContext>>,
    ctrl_ep_ring: Pin<Box<CommandRing>>,
    ep_desc_list: Vec<EndpointDescriptor>,
    ep_rings: [Option<TransferRing>; 32],
}
impl UsbDeviceDriverContext {
    pub async fn new(
        port: usize,
        slot: u8,
        xhci: Rc<Controller>,
        mut input_context: Pin<Box<InputContext>>,
        ctrl_ep_ring: Pin<Box<CommandRing>>,
        descriptors: Vec<UsbDescriptor>,
    ) -> Result<Self> {
        let mut ep_desc_list = Vec::new();
        for d in &descriptors {
            if let UsbDescriptor::Endpoint(e) = d {
                ep_desc_list.push(*e);
            }
        }
        let ep_rings = xhci
            .setup_endpoints(port, slot, &mut input_context.as_mut(), &ep_desc_list)
            .await?;
        Ok(Self {
            port,
            slot,
            xhci,
            descriptors,
            _input_context: input_context,
            ctrl_ep_ring,
            ep_desc_list,
            ep_rings,
        })
    }
    pub fn port(&self) -> usize {
        self.port
    }
    pub fn slot(&self) -> u8 {
        self.slot
    }
    pub fn xhci(&self) -> Rc<Controller> {
        self.xhci.clone()
    }
    pub fn descriptors(&self) -> &Vec<UsbDescriptor> {
        &self.descriptors
    }
    pub fn ep_desc_list(&self) -> &Vec<EndpointDescriptor> {
        &self.ep_desc_list
    }
    pub fn ep_ring(&self, dci: usize) -> Result<&Option<TransferRing>> {
        self.ep_rings
            .get(dci)
            .ok_or("dci out of range for ep_rings".into())
    }
    pub async fn set_config(&mut self, config_value: u8) -> Result<()> {
        self.xhci()
            .request_set_config(self.slot, &mut self.ctrl_ep_ring, config_value)
            .await
    }
    pub async fn set_interface(&mut self, interface_desc: &InterfaceDescriptor) -> Result<()> {
        self.xhci
            .request_set_interface(
                self.slot,
                &mut self.ctrl_ep_ring,
                interface_desc.interface_number(),
                interface_desc.alt_setting(),
            )
            .await
    }
    /// USB HID specific request
    pub async fn set_protocol(
        &mut self,
        interface_desc: &InterfaceDescriptor,
        protocol: UsbHidProtocol,
    ) -> Result<()> {
        self.xhci
            .request_set_protocol(
                self.slot,
                &mut self.ctrl_ep_ring,
                interface_desc.interface_number(),
                protocol as u8,
            )
            .await
    }
    pub fn push_trb_to_ctrl_ep(&mut self, trb: GenericTrbEntry) -> Result<u64> {
        self.ctrl_ep_ring.push(trb)
    }
    pub fn notify_ctrl_ep(&mut self) -> Result<()> {
        self.xhci.notify_ep(self.slot, 1)
    }
    pub fn notify_ep(&mut self, ep: &EndpointDescriptor) -> Result<()> {
        self.xhci.notify_ep(self.slot, ep.dci())
    }
    pub async fn wait_transfer_event(&mut self, trb_ptr_to_wait: u64) -> Result<()> {
        TransferEventFuture::new_with_timeout(
            self.xhci.primary_event_ring(),
            self.slot,
            trb_ptr_to_wait,
            10 * 1000,
        )
        .await?
        .ok_or(Error::Failed("Timed out"))?
        .completed()
    }
}
