extern crate alloc;

use crate::error::Result;
use crate::error::WasabiError;
use crate::println;
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

const ATTR_MASK: u64 = 0x0000_0000_0000_0FFF;
const ATTR_PRESENT: u64 = 1 << 0;
const ATTR_WRITABLE: u64 = 1 << 1;
const ATTR_USER: u64 = 1 << 2;
const ATTR_WRITE_THROUGH: u64 = 1 << 3;
const ATTR_CACHE_DISABLE: u64 = 1 << 4;

#[derive(Debug)]
#[repr(u64)]
pub enum PageAttr {
    NotPresent = 0,
    ReadWriteKernel = ATTR_PRESENT | ATTR_WRITABLE,
    ReadWriteUser = ATTR_PRESENT | ATTR_WRITABLE | ATTR_USER,
    ReadWriteIo = ATTR_PRESENT | ATTR_WRITABLE | ATTR_WRITE_THROUGH | ATTR_CACHE_DISABLE,
}

#[derive(Debug)]
#[repr(transparent)]
pub struct Entry<const LEVEL: &'static str, const SHIFT: usize> {
    value: u64,
}
impl<const LEVEL: &'static str, const SHIFT: usize> Entry<LEVEL, SHIFT> {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn write_value(&mut self, value: u64) {
        self.value = value;
    }
    fn format_additional_attrs(&self, _f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Result::Ok(())
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
            "{:9}Entry @ {:#p} {{ {:#018X} {}{}{} ",
            LEVEL,
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
impl<const LEVEL: &'static str, const SHIFT: usize> fmt::Display for Entry<LEVEL, SHIFT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

//
// Tables
//
pub struct Table<const LEVEL: &'static str, const SHIFT: usize> {
    pub entry: [Entry<LEVEL, SHIFT>; 512],
}
impl<const LEVEL: &'static str, const SHIFT: usize> Table<LEVEL, SHIFT> {
    const SHIFT: usize = SHIFT;
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{:5} @ {:#p} {{", LEVEL, self)?;
        for i in 0..512 {
            let e = &self.entry[i];
            if !e.is_present() {
                continue;
            }
            writeln!(f, "  entry[{:3}] = {:?}", i, e)?;
        }
        writeln!(f, "}}")
    }
    fn calc_index(addr: u64) -> usize {
        ((addr >> SHIFT) & 0b1_1111_1111) as usize
    }
}
impl<const LEVEL: &'static str, const SHIFT: usize> fmt::Debug for Table<LEVEL, SHIFT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

pub type PT = Table<"PT", 12>;
pub type PD = Table<"PD", 21>;
pub type PDPT = Table<"PDPT", 30>;
pub type PML4 = Table<"PML4", 39>;

impl PML4 {
    pub fn new() -> Box<Self> {
        Box::new(Self::default())
    }
    fn default() -> Self {
        // This is safe since entries filled with 0 is valid.
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
    pub fn create_mappng(
        &mut self,
        virt_start: u64,
        virt_end: u64,
        phys: u64,
        attr: PageAttr,
    ) -> Result<()> {
        println!("create_mappng(virt_start={virt_start:#018X}, virt_end={virt_end:#018X}, phys={phys:#018X}, attr={attr:?})");
        if virt_start & ATTR_MASK != 0 {
            return Err(WasabiError::Failed("Invalid virt_start"));
        }
        if virt_end & ATTR_MASK != 0 {
            return Err(WasabiError::Failed("Invalid virt_end"));
        }
        if phys & ATTR_MASK != 0 {
            return Err(WasabiError::Failed("Invalid phys"));
        }
        let mut ofs = 0;
        let index_range =
            Self::calc_index(virt_start)..Self::calc_index(virt_end + (1 << Self::SHIFT) - 1);
        println!("range: {:?}", index_range);
        for i in index_range {
            let e = &mut self.entry[i];
        }
        Ok(())
    }
}
