extern crate alloc;

use crate::error::Result;
use crate::error::WasabiError;
use crate::volatile::Volatile;
use crate::xhci::registers::UsbMode;
use crate::xhci::EndpointType;
use alloc::boxed::Box;
use alloc::fmt::Debug;
use alloc::format;
use core::marker::PhantomPinned;
use core::mem::size_of;
use core::mem::MaybeUninit;
use core::pin::Pin;

#[repr(C, align(32))]
#[derive(Default, Debug)]
pub struct EndpointContext {
    // data[0]:
    //   - bit[16..=23]: Interval (Table 6-12: Endpoint Type vs. Interval Calculation)
    // data[1]:
    //   - bit[1..=2]: Error Count (CErr)
    //   - bit[3..=5]: EndpointType (EPType)
    //   - bit[16..=31]: Max Packet Size (taken from EndpointDescriptor)
    data: [u32; 2],

    tr_dequeue_ptr: Volatile<u64>,

    // 4.14.1.1 (should be non-zero)
    average_trb_length: u16,

    // 4.14.2
    max_esit_payload_low: u16,
    _reserved: [u32; 3],
}
const _: () = assert!(size_of::<EndpointContext>() == 0x20);
impl EndpointContext {
    unsafe fn new() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
    pub fn new_interrupt_in_endpoint(
        max_packet_size: u16,
        tr_dequeue_ptr: u64,
        mode: UsbMode,
        interval_from_ep_desc: u8,
        average_trb_length: u16,
    ) -> Result<Self> {
        // See [xhci] Table 6-12
        let interval = match mode {
            UsbMode::LowSpeed | UsbMode::FullSpeed => {
                // interval_ms == 2^interval / 8 == interval_from_ep_desc
                interval_from_ep_desc.ilog2() as u8 + 3
            }
            UsbMode::HighSpeed | UsbMode::SuperSpeed => interval_from_ep_desc - 1,
            mode => {
                return Err(WasabiError::FailedString(format!(
                    "Failed to calc interval for {:?}",
                    mode
                )))
            }
        };
        let mut ep = unsafe { Self::new() };
        ep.set_ep_type(EndpointType::InterruptIn)?;
        ep.set_dequeue_cycle_state(true)?;
        ep.set_error_count(3)?;
        ep.set_max_packet_size(max_packet_size);
        ep.set_ring_dequeue_pointer(tr_dequeue_ptr)?;
        ep.data[0] = (interval as u32) << 16;
        ep.average_trb_length = average_trb_length;
        ep.max_esit_payload_low = max_packet_size; // 4.14.2
        Ok(ep)
    }
    pub fn new_control_endpoint(max_packet_size: u16, tr_dequeue_ptr: u64) -> Result<Self> {
        let mut ep = unsafe { Self::new() };
        ep.set_ep_type(EndpointType::Control)?;
        ep.set_dequeue_cycle_state(true)?;
        ep.set_error_count(3)?;
        ep.set_max_packet_size(max_packet_size);
        ep.set_ring_dequeue_pointer(tr_dequeue_ptr)?;
        ep.average_trb_length = 8; // 6.2.3: Software shall set Average TRB Length to ‘8’ for control endpoints.
        Ok(ep)
    }
    fn set_ep_type(&mut self, ep_type: EndpointType) -> Result<()> {
        let raw_ep_type = ep_type as u32;
        if raw_ep_type < 8 {
            self.data[1] &= !(0b111 << 3);
            self.data[1] |= raw_ep_type << 3;
            Ok(())
        } else {
            Err(WasabiError::FailedString(format!(
                "Invalid ep_type = {}",
                raw_ep_type
            )))
        }
    }
    fn set_ring_dequeue_pointer(&mut self, tr_dequeue_ptr: u64) -> Result<()> {
        self.tr_dequeue_ptr.write_bits(4, 60, tr_dequeue_ptr >> 4)
    }
    fn set_dequeue_cycle_state(&mut self, dcs: bool) -> Result<()> {
        self.tr_dequeue_ptr.write_bits(0, 1, dcs.into())
    }
    fn set_error_count(&mut self, error_count: u32) -> Result<()> {
        if error_count & !0b11 == 0 {
            self.data[1] &= !(0b11 << 1);
            self.data[1] |= error_count << 1;
            Ok(())
        } else {
            Err(WasabiError::Failed("invalid error_count"))
        }
    }
    fn set_max_packet_size(&mut self, max_packet_size: u16) {
        let max_packet_size = max_packet_size as u32;
        self.data[1] &= !(0xffff << 16);
        self.data[1] |= max_packet_size << 16;
    }
}

#[repr(C, align(32))]
#[derive(Default)]
pub struct DeviceContext {
    slot_ctx: [u32; 8],
    ep_ctx: [EndpointContext; 2 * 15 + 1],
}
const _: () = assert!(size_of::<DeviceContext>() == 0x400);
impl DeviceContext {
    fn set_root_hub_port_number(&mut self, port: usize) -> Result<()> {
        if 0 < port && port < 256 {
            self.slot_ctx[1] &= !(0xFF << 16);
            self.slot_ctx[1] |= (port as u32) << 16;
            Ok(())
        } else {
            Err(WasabiError::Failed("port out of range"))
        }
    }
    fn set_last_valid_dci(&mut self, dci: usize) -> Result<()> {
        // - 6.2.2:
        // ...the index (dci) of the last valid Endpoint Context
        // This field indicates the size of the Device Context structure.
        // For example, ((Context Entries+1) * 32 bytes) = Total bytes for this structure.
        // - 6.2.2.2:
        // A 'valid' Input Slot Context for a Configure Endpoint Command
        // requires the Context Entries field to be initialized to
        // the index of the last valid Endpoint Context that is
        // defined by the target configuration
        if dci <= 31 {
            self.slot_ctx[0] &= !(0b11111 << 27);
            self.slot_ctx[0] |= (dci as u32) << 27;
            Ok(())
        } else {
            Err(WasabiError::Failed("num_ep_ctx out of range"))
        }
    }
    fn set_port_speed(&mut self, mode: UsbMode) -> Result<()> {
        if mode.psi() < 16u32 {
            self.slot_ctx[0] &= !(0xF << 20);
            self.slot_ctx[0] |= (mode.psi()) << 20;
            Ok(())
        } else {
            Err(WasabiError::Failed("psi out of range"))
        }
    }
}

#[repr(C, align(32))]
#[derive(Default)]
pub struct InputControlContext {
    drop_context_bitmap: u32,
    add_context_bitmap: u32,
    data: [u32; 6],
}
const _: () = assert!(size_of::<InputControlContext>() == 0x20);
impl InputControlContext {
    pub fn add_context(&mut self, ici: usize) -> Result<()> {
        if ici < 32 {
            self.add_context_bitmap |= 1 << ici;
            Ok(())
        } else {
            Err(WasabiError::Failed("add_context: ici out of range"))
        }
    }
}

#[repr(C, align(4096))]
#[derive(Default)]
pub struct InputContext {
    input_ctrl_ctx: InputControlContext,
    device_ctx: DeviceContext,
    //
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<InputContext>() <= 4096);
impl InputContext {
    pub fn set_root_hub_port_number(self: &mut Pin<&mut Self>, port: usize) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut() }
            .device_ctx
            .set_root_hub_port_number(port)
    }
    pub fn set_last_valid_dci(self: &mut Pin<&mut Self>, dci: usize) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut() }
            .device_ctx
            .set_last_valid_dci(dci)
    }
    pub fn set_port_speed(self: &mut Pin<&mut Self>, psi: UsbMode) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut() }
            .device_ctx
            .set_port_speed(psi)
    }
    /// # Arguments
    /// * `dci` - destination device context index. [slot_ctx, ctrl_ep, ep1_out, ep1_in, ...]
    pub fn set_ep_ctx(
        self: &mut Pin<&mut Self>,
        dci: usize,
        ep_ctx: EndpointContext,
    ) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut().device_ctx.ep_ctx[dci - 1] = ep_ctx }
        Ok(())
    }
    pub fn set_input_ctrl_ctx(
        self: &mut Pin<&mut Self>,
        input_ctrl_ctx: InputControlContext,
    ) -> Result<()> {
        unsafe { self.as_mut().get_unchecked_mut().input_ctrl_ctx = input_ctrl_ctx }
        Ok(())
    }
}

#[repr(C, align(4096))]
#[derive(Default)]
pub struct OutputContext {
    device_ctx: DeviceContext,
    //
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<OutputContext>() <= 4096);

#[repr(C, align(64))]
pub struct RawDeviceContextBaseAddressArray {
    scratchpad_buffers: u64,
    context: [u64; 255],
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<RawDeviceContextBaseAddressArray>() == 2048);
impl RawDeviceContextBaseAddressArray {
    fn new() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}

pub struct DeviceContextBaseAddressArray {
    inner: Pin<Box<RawDeviceContextBaseAddressArray>>,
    context: [Option<Pin<Box<OutputContext>>>; 256],
    _scratchpad_buffers: Pin<Box<[*mut u8]>>,
}
impl DeviceContextBaseAddressArray {
    pub fn new(scratchpad_buffers: Pin<Box<[*mut u8]>>) -> Self {
        let mut inner = RawDeviceContextBaseAddressArray::new();
        inner.context[0] = scratchpad_buffers.as_ptr() as u64;
        Self {
            inner: Box::pin(inner),
            context: unsafe { MaybeUninit::zeroed().assume_init() },
            _scratchpad_buffers: scratchpad_buffers,
        }
    }
    /// # Safety
    /// This should only be called from set_dcbaa_ptr during the initialization
    pub unsafe fn inner_mut_ptr(&mut self) -> *mut RawDeviceContextBaseAddressArray {
        self.inner.as_mut().get_unchecked_mut() as *mut RawDeviceContextBaseAddressArray
    }
    pub fn set_output_context(&mut self, slot: u8, output_context: Pin<Box<OutputContext>>) {
        // Own the output context here
        let idx = (slot - 1) as usize;
        self.context[idx] = Some(output_context);
        // ...and set it in the actual pointer array
        unsafe {
            self.inner.as_mut().get_unchecked_mut().context[idx] =
                self.context[idx]
                    .as_ref()
                    .expect("Output Context was None")
                    .as_ref()
                    .get_ref() as *const OutputContext as u64;
        }
    }
}
