extern crate alloc;

use alloc::boxed::Box;
use core::arch::asm;
use core::fmt;
use core::mem::MaybeUninit;

pub fn read_cr3() -> *mut PML4 {
    let mut cr3: *mut PML4;
    unsafe {
        asm!("mov rax, cr3",
            out("rax") cr3)
    }
    cr3
}
/// # Safety
/// Writing to CR3 can causes any exceptions so it is
/// programmer's responsibility to setup correct page tables.
pub unsafe fn write_cr3(table: *const PML4) {
    asm!("mov cr3, rax",
            in("rax") table)
}

//
// Entries
//
pub trait PageTableEntry {
    const LEVEL: &'static str;
    fn read_value(&self) -> u64;
    fn write_value(&mut self, value: u64);
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
            "{:9} @ {:#p} {{ {:#018X} {}{}{} ",
            Self::LEVEL,
            self,
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
#[repr(transparent)]
pub struct PTEntry {
    value: u64,
}
impl PageTableEntry for PTEntry {
    const LEVEL: &'static str = "PTEntry";
    fn read_value(&self) -> u64 {
        self.value
    }
    fn write_value(&mut self, value: u64) {
        self.value = value;
    }
}
impl fmt::Display for PTEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct PDEntry {
    value: u64,
}
impl PageTableEntry for PDEntry {
    const LEVEL: &'static str = "PDEntry";
    fn read_value(&self) -> u64 {
        self.value
    }
    fn write_value(&mut self, value: u64) {
        self.value = value;
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
#[repr(transparent)]
pub struct PDPTEntry {
    value: u64,
}
impl PageTableEntry for PDPTEntry {
    const LEVEL: &'static str = "PDPTEntry";
    fn read_value(&self) -> u64 {
        self.value
    }
    fn write_value(&mut self, value: u64) {
        self.value = value;
    }
}
impl fmt::Display for PDPTEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct PML4Entry {
    value: u64,
}
impl PageTableEntry for PML4Entry {
    const LEVEL: &'static str = "PML4Entry";
    fn read_value(&self) -> u64 {
        self.value
    }
    fn write_value(&mut self, value: u64) {
        self.value = value;
    }
}
impl fmt::Display for PML4Entry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

//
// Tables
//
pub trait PageTable<T: 'static + PageTableEntry + core::fmt::Debug + core::fmt::Display> {
    const LEVEL: &'static str;
    fn read_entry(&self, index: usize) -> &T;
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{:5} @ {:#p} {{", Self::LEVEL, self)?;
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
#[repr(align(4096))]
pub struct PT {
    pub entry: [PTEntry; 512],
}
impl PageTable<PTEntry> for PT {
    const LEVEL: &'static str = "PT";
    fn read_entry(&self, index: usize) -> &PTEntry {
        &self.entry[index]
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
#[repr(align(4096))]
pub struct PD {
    pub entry: [PDEntry; 512],
}
impl PageTable<PDEntry> for PD {
    const LEVEL: &'static str = "PD";
    fn read_entry(&self, index: usize) -> &PDEntry {
        &self.entry[index]
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
#[repr(align(4096))]
pub struct PDPT {
    pub entry: [PDPTEntry; 512],
}
impl PageTable<PDPTEntry> for PDPT {
    const LEVEL: &'static str = "PDPT";
    fn read_entry(&self, index: usize) -> &PDPTEntry {
        &self.entry[index]
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

#[repr(align(4096))]
pub struct PML4 {
    pub entry: [PML4Entry; 512],
}
impl PML4 {
    pub fn new() -> Box<Self> {
        Box::new(Self::default())
    }
    fn default() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}
impl PageTable<PML4Entry> for PML4 {
    const LEVEL: &'static str = "PML4";
    fn read_entry(&self, index: usize) -> &PML4Entry {
        &self.entry[index]
    }
}
impl fmt::Debug for PML4 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
