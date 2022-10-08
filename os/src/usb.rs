use core::marker::PhantomPinned;
use core::mem::size_of;
use core::pin::Pin;
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

#[derive(Debug, Copy, Clone)]
pub enum UsbDescriptor {
    Device(DeviceDescriptor),
    Config(ConfigDescriptor),
    String,
    Interface(InterfaceDescriptor),
    Endpoint(EndpointDescriptor),
}

#[derive(Debug, Copy, Clone, Default)]
#[allow(unused)]
#[repr(packed)]
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

/// # Safety
/// Implementing this trait is safe only when the target type can be constructed from any byte
/// sequences that has the same size. If not, modification made via the byte slice produced by
/// as_mut_slice can be an undefined behavior since the bytes can not be interpreted as the
/// original type.
pub unsafe trait IntoPinnedMutableSlice: Sized {
    fn as_mut_slice(self: Pin<&mut Self>) -> Pin<&mut [u8]> {
        Pin::new(unsafe {
            slice::from_raw_parts_mut(
                self.get_unchecked_mut() as *mut Self as *mut u8,
                size_of::<Self>(),
            )
        })
    }
}
unsafe impl IntoPinnedMutableSlice for DeviceDescriptor {}
unsafe impl IntoPinnedMutableSlice for ConfigDescriptor {}

#[derive(Debug, Copy, Clone, Default)]
#[allow(unused)]
#[repr(packed)]
pub struct ConfigDescriptor {
    desc_length: u8,
    desc_type: u8,
    total_length: u16,
    num_of_interfaces: u8,
    config_value: u8,
    config_string_index: u8,
    attribute: u8,
    max_power: u8,
    //
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<ConfigDescriptor>() == 9);
impl ConfigDescriptor {
    pub fn total_length(&self) -> usize {
        self.total_length as usize
    }
}

#[derive(Debug, Copy, Clone, Default)]
#[allow(unused)]
#[repr(packed)]
pub struct InterfaceDescriptor {
    desc_length: u8,
    desc_type: u8,
    interface_number: u8,
    alt_setting: u8,
    num_of_endpoints: u8,
    interface_class: u8,
    interface_subclass: u8,
    interface_protocol: u8,
    interface_index: u8,
}
const _: () = assert!(size_of::<InterfaceDescriptor>() == 9);

#[derive(Debug, Copy, Clone, Default)]
#[allow(unused)]
#[repr(packed)]
pub struct EndpointDescriptor {
    desc_length: u8,
    desc_type: u8,
    endpoint_address: u8,
    attributes: u8,
    max_packet_size: u16,
    interval_ms: u8,
}
const _: () = assert!(size_of::<EndpointDescriptor>() == 7);
