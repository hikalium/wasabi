extern crate alloc;

pub struct UsbDeviceDriverContext {
    port: usize,
    slot: u8,
}
impl UsbDeviceDriverContext {
    pub fn new(port: usize, slot: u8) -> Self {
        Self {
            port, slot
        }
    }
    pub fn port(&self) -> usize {
        self.port
    }
    pub fn slot(&self) -> u8 {
        self.slot
    }
}
