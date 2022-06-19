use crate::println;
use core::arch::asm;
use core::fmt;

pub const MSR_IA32_APIC_BASE: u32 = 0x1b;

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
const _: () = assert!(core::mem::size_of::<Gdt>() / 8 == 6);
impl Gdt {
    pub fn load(&'static self) {
        let params = GdtrParameters {
            limit: (core::mem::size_of::<Gdt>() - 1) as u16,
            base: self,
        };
        unsafe {
            asm!("lgdt [rcx]",
                in("rcx") &params)
        }
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

pub mod segment_selector {
    use super::*;
    pub const KERNEL_CS: u16 = 1 << 3;
    pub const KERNEL_DS: u16 = 2 << 3;
    pub fn write_cs(cs: u16) {
        // The MOV instruction CANNOT be used to load the CS register.
        // Use far-jump(ljmp) instead.
        unsafe {
            asm!(
	"lea rax, [rip + 1f]", // Target address (label 1 below)
	"push cx", // Construct a far pointer on the stack
	"push rax",
	"ljmp [rsp]",
        "1:",
        "add rsp, 8 + 2", // Cleanup the far pointer on the stack
                in("cx") cs)
        }
    }
}

use attr_bits::*;
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

pub fn read_msr(port: u32) -> u64 {
    let mut high: u32;
    let mut low: u32;
    unsafe {
        asm!("rdmsr",
            in("ecx") port,
            out("edx") high,
            out("eax") low);
    }
    ((high as u64) << 32) | low as u64
}

pub fn write_msr(port: u32, data: u64) {
    unsafe {
        asm!("wrmsr",
            in("ecx") port,
            in("edx") (data >> 32),
            in("eax") data as u32);
    }
}

pub fn busy_loop_hint() {
    unsafe { asm!("pause") }
}

#[derive(Copy, Clone)]
pub struct CpuidRequest {
    pub eax: u32,
    pub ecx: u32,
}
impl fmt::Display for CpuidRequest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "CpuidRequest(EAX: {:#010X}, ECX:{:#010X})",
            self.eax, self.ecx,
        )
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct CpuidResponse {
    // Do not reorder the members!
    // This order matches the internal
    // encodings of the registers.
    ebx: u32,
    edx: u32,
    ecx: u32,
    eax: u32,
}
impl CpuidResponse {
    pub fn eax(&self) -> u32 {
        self.eax
    }
    pub fn ebx(&self) -> u32 {
        self.ebx
    }
    pub fn ecx(&self) -> u32 {
        self.ecx
    }
    pub fn edx(&self) -> u32 {
        self.edx
    }
}
impl fmt::Display for CpuidResponse {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "CpuidResponse(EAX: {:#010X}, EBX:{:#010X}, ECX:{:#010X}, EDX:{:#010X})",
            self.eax, self.ebx, self.ecx, self.edx
        )
    }
}

pub fn read_cpuid(request: CpuidRequest) -> CpuidResponse {
    let mut eax: u32 = request.eax;
    let mut ebx: u32;
    let mut ecx: u32 = request.ecx;
    let mut edx: u32;
    unsafe {
        asm!(
            "xchg rsi,rbx",
            "cpuid",
            "xchg rsi,rbx",
            inout("eax") eax,
            out("esi") ebx,
            inout("ecx") ecx,
            out("edx") edx,
            clobber_abi("C"),
        );
    }
    CpuidResponse { eax, ebx, ecx, edx }
}

pub fn write_io_port_u8(port: u16, data: u8) {
    unsafe {
        asm!("out dx, al",
            in("al") data,
            in("dx") port)
    }
}
pub fn write_io_port_u16(port: u16, data: u16) {
    unsafe {
        asm!("out dx, ax",
            in("ax") data,
            in("dx") port)
    }
}
pub fn write_io_port_u32(port: u16, data: u32) {
    unsafe {
        asm!("out dx, eax",
            in("eax") data,
            in("dx") port)
    }
}

pub fn read_io_port_u8(port: u16) -> u8 {
    let mut data: u8;
    unsafe {
        asm!("in al, dx",
            out("al") data,
            in("dx") port)
    }
    data
}
pub fn read_io_port_u16(port: u16) -> u16 {
    let mut data: u16;
    unsafe {
        asm!("in ax, dx",
            out("ax") data,
            in("dx") port)
    }
    data
}
pub fn read_io_port_u32(port: u16) -> u32 {
    let mut data: u32;
    unsafe {
        asm!("in eax, dx",
            out("eax") data,
            in("dx") port)
    }
    data
}

pub fn disable_legacy_pic() {
    // https://wiki.osdev.org/8259_PIC#Disabling
    write_io_port_u8(0xa1, 0xff);
    write_io_port_u8(0x21, 0xff);
    println!("Disabled legacy PIC");
}

pub fn hlt() {
    unsafe { asm!("hlt") }
}

pub fn rest_in_peace() {
    unsafe {
        loop {
            core::arch::asm!("cli;hlt;")
        }
    }
}

pub fn read_cr3() -> *mut PML4 {
    let mut cr3: *mut PML4;
    unsafe {
        asm!("mov rax, cr3",
            out("rax") cr3)
    }
    cr3
}

pub trait PageTableEntry {
    fn read_value(&self) -> u64;
    fn read_name() -> &'static str;
    fn format_additional_attrs(&self, _f: &mut fmt::Formatter) -> fmt::Result {
        Result::Ok(())
    }
    fn is_present(&self) -> bool {
        (self.read_value() & (1 << 0)) != 0
    }
    fn is_writable(&self) -> bool {
        (self.read_value() & (1 << 1)) != 0
    }
    fn is_user(&self) -> bool {
        (self.read_value() & (1 << 2)) != 0
    }
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:9} {{ {:#018X} {}{}{} ",
            Self::read_name(),
            self.read_value(),
            if self.is_present() { "P" } else { "N" },
            if self.is_writable() { "W" } else { "R" },
            if self.is_user() { "U" } else { "S" }
        )?;
        self.format_additional_attrs(f)?;
        write!(f, " }}")
    }
}

#[derive(Debug)]
pub struct PTEntry {
    value: u64,
}
impl PageTableEntry for PTEntry {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn read_name() -> &'static str {
        "PTEntry"
    }
}
impl fmt::Display for PTEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

#[derive(Debug)]
pub struct PDEntry {
    value: u64,
}
impl PageTableEntry for PDEntry {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn read_name() -> &'static str {
        "PDEntry"
    }
    fn format_additional_attrs(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.read_value() & 0x80 != 0 {
            write!(f, "2MBPage")?;
        }
        Result::Ok(())
    }
}
impl fmt::Display for PDEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

#[derive(Debug)]
pub struct PDPTEntry {
    value: u64,
}
impl PageTableEntry for PDPTEntry {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn read_name() -> &'static str {
        "PDPTEntry"
    }
}
impl fmt::Display for PDPTEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

#[derive(Debug)]
pub struct PML4Entry {
    value: u64,
}
impl PageTableEntry for PML4Entry {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn read_name() -> &'static str {
        "PML4Entry"
    }
}
impl fmt::Display for PML4Entry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

pub trait PageTable<T: 'static + PageTableEntry + core::fmt::Debug + core::fmt::Display> {
    fn read_entry(&self, index: usize) -> &T;
    fn read_name() -> &'static str;
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{:5} @ {:#p} {{", Self::read_name(), self)?;
        for i in 0..512 {
            let e = self.read_entry(i);
            if !e.is_present() {
                continue;
            }
            writeln!(f, "  entry[{:3}] = {}", i, e)?;
        }
        writeln!(f, "}}")?;
        Result::Ok(())
    }
}
#[derive(Debug)]
pub struct PT {
    pub entry: [PTEntry; 512],
}
impl PageTable<PTEntry> for PT {
    fn read_entry(&self, index: usize) -> &PTEntry {
        &self.entry[index]
    }
    fn read_name() -> &'static str {
        "PT"
    }
}
impl fmt::Display for PT {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
pub fn get_pt(e: &PDEntry) -> &'static mut PT {
    unsafe { &mut *(((e.read_value() & !0xFFF) as usize) as *mut PT) }
}

#[derive(Debug)]
pub struct PD {
    pub entry: [PDEntry; 512],
}
impl PageTable<PDEntry> for PD {
    fn read_entry(&self, index: usize) -> &PDEntry {
        &self.entry[index]
    }
    fn read_name() -> &'static str {
        "PD"
    }
}
impl fmt::Display for PD {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
pub fn get_pd(e: &PDPTEntry) -> &'static mut PD {
    unsafe { &mut *(((e.read_value() & !0xFFF) as usize) as *mut PD) }
}

#[derive(Debug)]
pub struct PDPT {
    pub entry: [PDPTEntry; 512],
}
impl PageTable<PDPTEntry> for PDPT {
    fn read_entry(&self, index: usize) -> &PDPTEntry {
        &self.entry[index]
    }
    fn read_name() -> &'static str {
        "PDPT"
    }
}
impl fmt::Display for PDPT {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
pub fn get_pdpt(e: &PML4Entry) -> &'static mut PDPT {
    unsafe { &mut *(((e.read_value() & !0xFFF) as usize) as *mut PDPT) }
}

#[derive(Debug)]
pub struct PML4 {
    pub entry: [PML4Entry; 512],
}
impl PageTable<PML4Entry> for PML4 {
    fn read_entry(&self, index: usize) -> &PML4Entry {
        &self.entry[index]
    }
    fn read_name() -> &'static str {
        "PML4"
    }
}
impl fmt::Display for PML4 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
