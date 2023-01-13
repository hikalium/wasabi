extern crate alloc;

use crate::error::Result;
use crate::println;
use crate::x86_64::idt::TaskStateSegment64;
use crate::x86_64::TSS64_SEL;
use alloc::boxed::Box;
use core::arch::asm;
use core::fmt;
use core::mem::size_of;
use core::pin::Pin;

pub const BIT_TYPE_DATA: u64 = 0b10u64 << 43;
pub const BIT_TYPE_CODE: u64 = 0b11u64 << 43;

pub const BIT_PRESENT: u64 = 1u64 << 47;
pub const BIT_CS_LONG_MODE: u64 = 1u64 << 53;
pub const BIT_CS_READABLE: u64 = 1u64 << 53;
pub const BIT_DS_WRITABLE: u64 = 1u64 << 41;
pub const BIT_DPL0: u64 = 0u64 << 45;
pub const BIT_DPL3: u64 = 3u64 << 45;

#[repr(u64)]
enum GdtAttr {
    KernelCode = BIT_TYPE_CODE | BIT_PRESENT | BIT_CS_LONG_MODE | BIT_CS_READABLE,
    KernelData = BIT_TYPE_DATA | BIT_PRESENT | BIT_DS_WRITABLE,
    User64Code = BIT_TYPE_CODE | BIT_PRESENT | BIT_CS_LONG_MODE | BIT_CS_READABLE | BIT_DPL3,
    UserData = BIT_TYPE_DATA | BIT_PRESENT | BIT_DS_WRITABLE | BIT_DPL3,
}

#[allow(dead_code)]
#[repr(packed)]
struct GdtrParameters {
    limit: u16,
    base: *const Gdt,
}

#[allow(dead_code)]
#[repr(packed)]
pub struct Gdt {
    null_segment: GdtSegmentDescriptor,
    kernel_code_segment: GdtSegmentDescriptor,
    kernel_data_segment: GdtSegmentDescriptor,
    user_code_segment_32: GdtSegmentDescriptor,
    user_data_segment: GdtSegmentDescriptor,
    user_code_segment_64: GdtSegmentDescriptor,
    task_state_segment: TaskStateSegment64Descriptor,
}
const _: () = assert!(size_of::<Gdt>() == 64);

impl Gdt {
    pub fn new(tss64: &Pin<Box<TaskStateSegment64>>) -> Result<Pin<Box<Gdt>>> {
        let gdt = Self {
            null_segment: GdtSegmentDescriptor::null(),
            kernel_code_segment: GdtSegmentDescriptor::new(GdtAttr::KernelCode),
            kernel_data_segment: GdtSegmentDescriptor::new(GdtAttr::KernelData),
            user_code_segment_32: GdtSegmentDescriptor::null(),
            user_data_segment: GdtSegmentDescriptor::new(GdtAttr::UserData),
            user_code_segment_64: GdtSegmentDescriptor::new(GdtAttr::User64Code),
            task_state_segment: TaskStateSegment64Descriptor::new(tss64.phys_addr()),
        };
        let gdt = Box::pin(gdt);
        let params = GdtrParameters {
            limit: (size_of::<Gdt>() - 1) as u16,
            base: gdt.as_ref().get_ref() as *const Gdt,
        };
        println!("Loading GDT @ {:#018X}", params.base as u64);
        // SAFETY: This is safe since it is loading a valid GDT just constructed in the above
        unsafe {
            asm!("lgdt [rcx]",
                in("rcx") &params);
        }
        println!("Loading TSS ( selector = {:#X} )", TSS64_SEL);
        unsafe {
            asm!("ltr cx",
                in("cx") TSS64_SEL);
        }
        Ok(gdt)
    }
}

pub struct GdtSegmentDescriptor {
    value: u64,
}
impl GdtSegmentDescriptor {
    const fn null() -> Self {
        Self { value: 0 }
    }
    const fn new(attr: GdtAttr) -> Self {
        Self { value: attr as u64 }
    }
}
impl fmt::Display for GdtSegmentDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:#18X}", self.value)
    }
}

#[repr(packed)]
#[allow(dead_code)]
struct TaskStateSegment64Descriptor {
    limit_low: u16,
    base_low: u16,
    base_mid_low: u8,
    attr: u16,
    base_mid_high: u8,
    base_high: u32,
    reserved: u32,
}
impl TaskStateSegment64Descriptor {
    const fn new(base_addr: u64) -> Self {
        Self {
            limit_low: size_of::<TaskStateSegment64>() as u16,
            base_low: (base_addr & 0xffff) as u16,
            base_mid_low: ((base_addr >> 16) & 0xff) as u8,
            attr: 0b1000_0000_1000_1001,
            base_mid_high: ((base_addr >> 24) & 0xff) as u8,
            base_high: ((base_addr >> 32) & 0xffffffff) as u32,
            reserved: 0,
        }
    }
}
const _: () = assert!(size_of::<TaskStateSegment64Descriptor>() == 16);
