use crate::error::Result;
use crate::error::WasabiError;
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
    Unknown { desc_len: u8, desc_type: u8 },
}

pub struct DescriptorIterator<'a> {
    buf: &'a [u8],
    index: usize,
}
impl<'a> DescriptorIterator<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, index: 0 }
    }
}
impl<'a> Iterator for DescriptorIterator<'a> {
    type Item = UsbDescriptor;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.buf.len() {
            None
        } else {
            let buf = &self.buf[self.index..];
            let desc_len = buf[0];
            let desc_type = buf[1];
            let desc = match desc_type {
                e if e == DescriptorType::Config as u8 => {
                    UsbDescriptor::Config(ConfigDescriptor::copy_from_slice(buf).ok()?)
                }
                e if e == DescriptorType::Interface as u8 => {
                    UsbDescriptor::Interface(InterfaceDescriptor::copy_from_slice(buf).ok()?)
                }
                e if e == DescriptorType::Endpoint as u8 => {
                    UsbDescriptor::Endpoint(EndpointDescriptor::copy_from_slice(buf).ok()?)
                }
                _ => UsbDescriptor::Unknown {
                    desc_len,
                    desc_type,
                },
            };
            self.index += desc_len as usize;
            Some(desc)
        }
    }
}

#[derive(Debug, Copy, Clone, Default)]
#[allow(unused)]
#[repr(packed)]
pub struct DeviceDescriptor {
    pub desc_length: u8,
    pub desc_type: u8,
    pub version: u16,
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub max_packet_size: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_version: u16,
    pub manufacturer_idx: u8,
    pub product_idx: u8,
    pub serial_idx: u8,
    pub num_of_config: u8,
}
const _: () = assert!(size_of::<DeviceDescriptor>() == 18);

/// # Safety
/// Implementing this trait is safe only when the target type can be constructed from any byte
/// sequences that has the same size. If not, modification made via the byte slice produced by
/// as_mut_slice can be an undefined behavior since the bytes can not be interpreted as the
/// original type.
pub unsafe trait IntoPinnedMutableSlice: Sized + Copy + Clone {
    fn as_mut_slice(self: Pin<&mut Self>) -> Pin<&mut [u8]> {
        Pin::new(unsafe {
            slice::from_raw_parts_mut(
                self.get_unchecked_mut() as *mut Self as *mut u8,
                size_of::<Self>(),
            )
        })
    }
    fn copy_from_slice(data: &[u8]) -> Result<Self> {
        if size_of::<Self>() > data.len() {
            Err(WasabiError::Failed("data is too short"))
        } else {
            Ok(unsafe { *(data.as_ptr() as *const Self) })
        }
    }
}
unsafe impl IntoPinnedMutableSlice for DeviceDescriptor {}
unsafe impl IntoPinnedMutableSlice for ConfigDescriptor {}
unsafe impl IntoPinnedMutableSlice for InterfaceDescriptor {}
unsafe impl IntoPinnedMutableSlice for EndpointDescriptor {}

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
    pub fn config_value(&self) -> u8 {
        self.config_value
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
impl InterfaceDescriptor {
    pub fn triple(&self) -> (u8, u8, u8) {
        (
            self.interface_class,
            self.interface_subclass,
            self.interface_protocol,
        )
    }
    pub fn interface_number(&self) -> u8 {
        self.interface_number
    }
    pub fn alt_setting(&self) -> u8 {
        self.alt_setting
    }
}

#[derive(Debug, Copy, Clone, Default)]
#[allow(unused)]
#[repr(packed)]
pub struct EndpointDescriptor {
    desc_length: u8,
    desc_type: u8,

    // endpoint_address:
    //   - bit[0..=3]: endpoint number
    //   - bit[7]: direction(0: out, 1: in)
    endpoint_address: u8,

    // attributes:
    //   - bit[0..=1]: transfer type(0: Control, 1: Isochronous, 2: Bulk, 3: Interrupt)
    attributes: u8,
    max_packet_size: u16,
    interval_ms: u8,
}
const _: () = assert!(size_of::<EndpointDescriptor>() == 7);
