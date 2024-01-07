extern crate alloc;

use crate::allocator::ALLOCATOR;
use crate::error::Error;
use crate::error::Result;
use crate::memory::Mmio;
use crate::mutex::Mutex;
use crate::pci::BusDeviceFunction;
use crate::pci::Pci;
use crate::util::PAGE_SIZE;
use crate::xhci::context::DeviceContextBaseAddressArray;
use crate::xhci::controller::Controller;
use crate::xhci::registers::CapabilityRegisters;
use crate::xhci::registers::Doorbell;
use crate::xhci::registers::OperationalRegisters;
use crate::xhci::registers::PortSc;
use crate::xhci::registers::RuntimeRegisters;
use alloc::alloc::Layout;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::convert::AsRef;
use core::mem::size_of;
use core::pin::Pin;
use core::slice;

// Allocate scratchpad buffers as xHCI spec says:
// > 4.20 Scratchpad Buffers
// > This should be done before xHC starts.
// > device_contexts.context[0] points Scratchpad Buffer
// > Arrary. The array contains pointers to a memory region
// > which is sized PAGESIZE and aligned on
// > PAGESIZE. (PAGESIZE can be retrieved from
// > op_regs.PAGESIZE)
fn alloc_scratch_pad_buffers(num_scratch_pad_bufs: usize) -> Result<Pin<Box<[*mut u8]>>> {
    let scratchpad_buffers = ALLOCATOR.alloc_with_options(
        Layout::from_size_align(size_of::<usize>() * num_scratch_pad_bufs, PAGE_SIZE)
            .map_err(|_| Error::Failed("could not allocated scratchpad buffers"))?,
    );
    let scratchpad_buffers =
        unsafe { slice::from_raw_parts(scratchpad_buffers as *mut *mut u8, num_scratch_pad_bufs) };
    let mut scratchpad_buffers = Pin::new(Box::<[*mut u8]>::from(scratchpad_buffers));
    for sb in scratchpad_buffers.iter_mut() {
        *sb = ALLOCATOR.alloc_with_options(
            Layout::from_size_align(PAGE_SIZE, PAGE_SIZE)
                .map_err(|_| Error::Failed("could not allocated scratchpad buffers"))?,
        );
    }
    Ok(scratchpad_buffers)
}

pub fn create_host_controller(bdf: BusDeviceFunction) -> Result<Controller> {
    let pci = Pci::take();
    pci.disable_interrupt(bdf)?;
    pci.enable_bus_master(bdf)?;
    let bar0 = pci.try_bar0_mem64(bdf)?;
    bar0.disable_cache();

    let cap_regs = unsafe { Mmio::from_raw(bar0.addr() as *mut CapabilityRegisters) };
    cap_regs.as_ref().assert_capabilities()?;

    let mut op_regs = unsafe {
        Mmio::from_raw(bar0.addr().add(cap_regs.as_ref().length()) as *mut OperationalRegisters)
    };
    unsafe { op_regs.get_unchecked_mut() }.reset_xhc();
    op_regs.as_ref().assert_params()?;

    let rt_regs = unsafe {
        Mmio::from_raw(bar0.addr().add(cap_regs.as_ref().rtsoff()) as *mut RuntimeRegisters)
    };

    let num_slots = cap_regs.as_ref().num_of_device_slots();
    let mut doorbell_regs = Vec::new();
    for i in 0..=num_slots {
        let ptr = unsafe { bar0.addr().add(cap_regs.as_ref().dboff()).add(4 * i) as *mut u32 };
        doorbell_regs.push(Rc::new(Doorbell::new(ptr)))
    }
    // number of doorbells will be 1 + num_slots since doorbell[] is for the host controller.
    assert!(doorbell_regs.len() == 1 + num_slots);

    let portsc = PortSc::new(&bar0, cap_regs.as_ref());
    let scratchpad_buffers = alloc_scratch_pad_buffers(cap_regs.as_ref().num_scratch_pad_bufs())?;
    let device_context_base_array = DeviceContextBaseAddressArray::new(scratchpad_buffers);
    let device_context_base_array =
        Mutex::new(device_context_base_array, "Xhci.device_context_base_array");
    Controller::new(
        cap_regs,
        op_regs,
        rt_regs,
        portsc,
        doorbell_regs,
        device_context_base_array,
    )
}
