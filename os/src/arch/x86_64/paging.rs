extern crate alloc;

use crate::error::Result;
use crate::error::WasabiError;
use crate::println;
use crate::util::PAGE_SIZE;
use alloc::boxed::Box;
use core::arch::asm;
use core::fmt;
use core::marker::PhantomData;
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
// Access rights will be ANDed over all levels
const ATTR_PRESENT: u64 = 1 << 0;
const ATTR_WRITABLE: u64 = 1 << 1;
const ATTR_USER: u64 = 1 << 2;
// Cache control is only effective for the region referred by the entry
const ATTR_WRITE_THROUGH: u64 = 1 << 3;
const ATTR_CACHE_DISABLE: u64 = 1 << 4;

#[derive(Debug, Copy, Clone)]
#[repr(u64)]
pub enum PageAttr {
    NotPresent = 0,
    ReadWriteKernel = ATTR_PRESENT | ATTR_WRITABLE,
    ReadWriteUser = ATTR_PRESENT | ATTR_WRITABLE | ATTR_USER,
    ReadWriteIo = ATTR_PRESENT | ATTR_WRITABLE | ATTR_WRITE_THROUGH | ATTR_CACHE_DISABLE,
}

#[repr(transparent)]
pub struct Entry<const LEVEL: &'static str, const SHIFT: usize, NEXT> {
    value: u64,
    next_type: PhantomData<NEXT>,
}
impl<const LEVEL: &'static str, const SHIFT: usize, NEXT> Entry<LEVEL, SHIFT, NEXT> {
    fn read_value(&self) -> u64 {
        self.value
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
            "{}Entry @ {:#p} {{ {:#018X} {}{}{} ",
            LEVEL,
            self,
            self.read_value(),
            if self.is_present() { "P" } else { "N" },
            if self.is_writable() { "W" } else { "R" },
            if self.is_user() { "U" } else { "S" }
        )?;
        write!(f, " }}")
    }
    fn table_mut(&mut self) -> Result<&mut NEXT> {
        if self.is_present() {
            Ok(unsafe { &mut *((self.value & !ATTR_MASK) as *mut NEXT) })
        } else {
            Err(WasabiError::Failed("Next level is not present"))
        }
    }
    fn populate(&mut self) -> Result<&mut Self> {
        if self.is_present() {
            Err(WasabiError::Failed("Page is already populated"))
        } else {
            let next: Box<NEXT> = Box::new(unsafe { MaybeUninit::zeroed().assume_init() });
            self.value = Box::into_raw(next) as u64 | PageAttr::ReadWriteUser as u64;
            Ok(self)
        }
    }
    fn ensure_populated(&mut self) -> Result<&mut Self> {
        if self.is_present() {
            Ok(self)
        } else {
            self.populate()
        }
    }
    fn set_page(&mut self, phys: u64, attr: PageAttr) -> Result<()> {
        if self.is_present() {
            Err(WasabiError::Failed("Page is already populated"))
        } else if phys & ATTR_MASK != 0 {
            Err(WasabiError::Failed("phys is not aligned"))
        } else {
            self.value = phys | attr as u64;
            Ok(())
        }
    }
}

impl<const LEVEL: &'static str, const SHIFT: usize, NEXT> fmt::Display
    for Entry<LEVEL, SHIFT, NEXT>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
impl<const LEVEL: &'static str, const SHIFT: usize, NEXT> fmt::Debug for Entry<LEVEL, SHIFT, NEXT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

//
// Tables
//
#[repr(align(4096))]
pub struct Table<const LEVEL: &'static str, const SHIFT: usize, NEXT> {
    entry: [Entry<LEVEL, SHIFT, NEXT>; 512],
}
impl<const LEVEL: &'static str, const SHIFT: usize, NEXT: core::fmt::Debug>
    Table<LEVEL, SHIFT, NEXT>
{
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
    fn calc_index(&self, addr: u64) -> usize {
        ((addr >> SHIFT) & 0b1_1111_1111) as usize
    }
}
impl<const LEVEL: &'static str, const SHIFT: usize, NEXT: fmt::Debug> fmt::Debug
    for Table<LEVEL, SHIFT, NEXT>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

pub type PT = Table<"PT", 12, [u8; PAGE_SIZE]>;
pub type PD = Table<"PD", 21, PT>;
pub type PDPT = Table<"PDPT", 30, PD>;
pub type PML4 = Table<"PML4", 39, PDPT>;

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
        for addr in (virt_start..virt_end).step_by(PAGE_SIZE) {
            let index = self.calc_index(addr);
            let table = self.entry[index].ensure_populated()?.table_mut()?;
            let index = table.calc_index(addr);
            let table = table.entry[index].ensure_populated()?.table_mut()?;
            let index = table.calc_index(addr);
            let table = table.entry[index].ensure_populated()?.table_mut()?;
            let index = table.calc_index(addr);
            let pte = &mut table.entry[index];
            pte.set_page(phys + addr - virt_start, attr)?;
        }
        Ok(())
    }
}
