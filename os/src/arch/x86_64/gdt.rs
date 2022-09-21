use attr_bits::BIT_CS_LONG_MODE;
use attr_bits::BIT_CS_READABLE;
use attr_bits::BIT_DS_WRITABLE;
use attr_bits::BIT_PRESENT;
use attr_bits::BIT_TYPE_CODE;
use attr_bits::BIT_TYPE_DATA;
use core::arch::asm;
use core::fmt;
use core::mem::size_of;

#[allow(dead_code)]
#[repr(packed)]
struct GdtrParameters {
    limit: u16,
    base: &'static Gdt,
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
}
const _: () = assert!(size_of::<Gdt>() / 8 == 6);
impl Gdt {
    /// # Safety
    /// Anything can happen if the GDT given is invalid
    /// and latter segment register modification does something
    /// that is not matched with the GDT.
    pub unsafe fn load(&'static self) {
        let params = GdtrParameters {
            limit: (size_of::<Gdt>() - 1) as u16,
            base: self,
        };
        asm!("lgdt [rcx]",
                in("rcx") &params)
    }
}

pub static GDT: Gdt = Gdt {
    null_segment: GdtSegmentDescriptor::null(),
    kernel_code_segment: GdtSegmentDescriptor::new(GdtAttr::KernelCode),
    kernel_data_segment: GdtSegmentDescriptor::new(GdtAttr::KernelData),
    user_code_segment_32: GdtSegmentDescriptor::null(),
    user_data_segment: GdtSegmentDescriptor::null(),
    user_code_segment_64: GdtSegmentDescriptor::null(),
};

mod attr_bits {
    pub const BIT_TYPE_DATA: u64 = 0b10u64 << 43;
    pub const BIT_TYPE_CODE: u64 = 0b11u64 << 43;

    pub const BIT_PRESENT: u64 = 1u64 << 47;
    pub const BIT_CS_LONG_MODE: u64 = 1u64 << 53;
    pub const BIT_CS_READABLE: u64 = 1u64 << 53;
    pub const BIT_DS_WRITABLE: u64 = 1u64 << 41;
}

#[repr(u64)]
enum GdtAttr {
    KernelCode = BIT_TYPE_CODE | BIT_PRESENT | BIT_CS_LONG_MODE | BIT_CS_READABLE,
    KernelData = BIT_TYPE_DATA | BIT_PRESENT | BIT_DS_WRITABLE,
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
