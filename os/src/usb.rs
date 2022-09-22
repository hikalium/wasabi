use core::mem::size_of;
use core::slice;

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
#[non_exhaustive]
#[allow(unused)]
#[derive(PartialEq, Eq)]
pub enum DescriptorType {
    Device = 1,
    Config = 2,
    String = 3,
    Interface = 4,
    Endpoint = 5,
}

#[derive(Debug, Copy, Clone, Default)]
#[allow(unused)]
#[repr(C)]
pub struct DeviceDescriptor {
    desc_length: u8,
    desc_type: u8,
    version: u16,
    device_class: u8,
    device_subclass: u8,
    device_protocol: u8,
    max_packet_size: u8,
    vendor_id: u16,
    product_id: u16,
    device_version: u16,
    manufacturer_idx: u8,
    product_idx: u8,
    serial_idx: u8,
    num_of_config: u8,
}
const _: () = assert!(size_of::<DeviceDescriptor>() == 18);
impl DeviceDescriptor {
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self as *mut Self as *mut u8, size_of::<Self>()) }
    }
}
