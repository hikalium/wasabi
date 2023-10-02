extern crate alloc;

use crate::usb::UsbDescriptor;
use alloc::vec::Vec;

pub struct UsbDeviceDriverContext {
    port: usize,
    slot: u8,
    descriptors: Vec<UsbDescriptor>,
}
impl UsbDeviceDriverContext {
    pub fn new(port: usize, slot: u8, descriptors: Vec<UsbDescriptor>) -> Self {
        Self {
            port,
            slot,
            descriptors,
        }
    }
    pub fn port(&self) -> usize {
        self.port
    }
    pub fn slot(&self) -> u8 {
        self.slot
    }
    pub fn descriptors(&self) -> &Vec<UsbDescriptor> {
        &self.descriptors
    }
}
