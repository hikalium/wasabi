extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::util::PAGE_SIZE;
use alloc::boxed::Box;
use core::arch::asm;
use core::fmt;
use core::marker::PhantomData;
use core::marker::PhantomPinned;
use core::mem::size_of_val;
use core::mem::ManuallyDrop;
use core::mem::MaybeUninit;
use core::pin::Pin;

pub struct Mmio<T: Sized> {
    inner: ManuallyDrop<Pin<Box<T>>>,
}
impl<T: Sized> Mmio<T> {
    /// # Safety
    /// Caller must ensure:
    /// - *ptr is valid
    /// - CPU Caches for the range pointed by ptr are disabled
    /// - No other party in this program have the ownership of *ptr
    pub unsafe fn from_raw(ptr: *mut T) -> Self {
        Self {
            inner: ManuallyDrop::new(Box::into_pin(Box::from_raw(ptr))),
        }
    }
    /// # Safety
    /// Same rules as Pin::get_unchecked_mut() applies.
    pub unsafe fn get_unchecked_mut(&mut self) -> &mut T {
        self.inner.as_mut().get_unchecked_mut()
    }
}
impl<T> AsRef<T> for Mmio<T> {
    fn as_ref(&self) -> &T {
        self.inner.as_ref().get_ref()
    }
}

#[repr(align(4096))]
pub struct IoBoxInner<T: Sized> {
    data: T,
    _pinned: PhantomPinned,
}
impl<T: Sized> IoBoxInner<T> {
    pub fn new(data: T) -> Self {
        Self {
            data,
            _pinned: PhantomPinned,
        }
    }
}

pub struct IoBox<T: Sized> {
    inner: Pin<Box<IoBoxInner<T>>>,
}
impl<T: Sized> IoBox<T> {
    pub fn new() -> Self {
        let inner = Box::pin(IoBoxInner::new(unsafe {
            MaybeUninit::<T>::zeroed().assume_init()
        }));
        let this = Self { inner };
        disable_cache(&this);
        this
    }
    /// # Safety
    /// Same rules as Pin::get_unchecked_mut() applies.
    pub unsafe fn get_unchecked_mut(&mut self) -> &mut T {
        &mut self.inner.as_mut().get_unchecked_mut().data
    }
}
impl<T> AsRef<T> for IoBox<T> {
    fn as_ref(&self) -> &T {
        &self.inner.as_ref().get_ref().data
    }
}
impl<T: Sized> Default for IoBox<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[test_case]
fn io_box_addr() {
    IoBox::<u64>::new();
}

pub fn disable_cache<T: Sized>(io_box: &IoBox<T>) {
    let region = io_box.inner.as_ref().get_ref();
    let vstart = region as *const IoBoxInner<T> as u64;
    let vend = vstart + size_of_val(region) as u64;
    unsafe {
        with_current_page_table(|pt| {
            pt.create_mapping(vstart, vend, vstart, PageAttr::ReadWriteIo)
                .expect("Failed to create mapping")
        })
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
/// # Safety
/// Writing to CR3 can causes any exceptions so it is
/// programmer's responsibility to setup correct page tables.
pub unsafe fn write_cr3(table: *const PML4) {
    asm!("mov cr3, rax",
            in("rax") table)
}

/// # Safety
/// This will create a mutable reference to the page table structure
/// So is is programmer's responsibility to ensure that at most one
/// instance of the reference exist at every moment.
pub unsafe fn take_current_page_table() -> ManuallyDrop<Box<PML4>> {
    ManuallyDrop::new(Box::from_raw(read_cr3()))
}
/// # Safety
/// This function sets the CR3 value so that anything bad can happen.
pub unsafe fn put_current_page_table(mut table: ManuallyDrop<Box<PML4>>) {
    // Set CR3 to reflect the updates and drop TLB caches.
    write_cr3(Box::into_raw(ManuallyDrop::take(&mut table)))
}
/// # Safety
/// This function modifies the page table as callback does, so
/// anything bad can happen if there are some mistakes.
pub unsafe fn with_current_page_table<F>(callback: F)
where
    F: FnOnce(&mut PML4),
{
    let mut table = take_current_page_table();
    callback(&mut table);
    put_current_page_table(table)
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
#[derive(Debug, Eq, PartialEq)]
pub enum TranslationResult {
    PageMapped4K { phys: u64 },
    PageMapped2M { phys: u64 },
    PageMapped1G { phys: u64 },
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
    fn table(&self) -> Result<&NEXT> {
        if self.is_present() {
            Ok(unsafe { &*((self.value & !ATTR_MASK) as *const NEXT) })
        } else {
            Err(Error::PageNotFound)
        }
    }
    fn table_mut(&mut self) -> Result<&mut NEXT> {
        if self.is_present() {
            Ok(unsafe { &mut *((self.value & !ATTR_MASK) as *mut NEXT) })
        } else {
            Err(Error::PageNotFound)
        }
    }
    fn page(&self) -> Result<u64> {
        if self.is_present() {
            Ok(self.value & !ATTR_MASK)
        } else {
            Err(Error::PageNotFound)
        }
    }
    fn populate(&mut self) -> Result<&mut Self> {
        if self.is_present() {
            Err(Error::Failed("Page is already populated"))
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
        if phys & ATTR_MASK != 0 {
            Err(Error::Failed("phys is not aligned"))
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
    pub fn create_mapping(
        &mut self,
        virt_start: u64,
        virt_end: u64,
        phys: u64,
        attr: PageAttr,
    ) -> Result<()> {
        if virt_start & ATTR_MASK != 0 {
            return Err(Error::Failed("Invalid virt_start"));
        }
        if virt_end & ATTR_MASK != 0 {
            return Err(Error::Failed("Invalid virt_end"));
        }
        if phys & ATTR_MASK != 0 {
            return Err(Error::Failed("Invalid phys"));
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
    pub fn translate(&self, virt: u64) -> Result<TranslationResult> {
        let index = self.calc_index(virt);
        let entry = &self.entry[index];
        let table = entry.table()?;

        let index = table.calc_index(virt);
        let entry = &table.entry[index];
        let table = entry.table()?;

        let index = table.calc_index(virt);
        let entry = &table.entry[index];
        let table = entry.table()?;

        let index = table.calc_index(virt);
        let entry = &table.entry[index];
        let page = entry.page()?;

        Ok(TranslationResult::PageMapped4K { phys: page })
    }
}

#[test_case]
fn page_translation() {
    use TranslationResult::*;
    // Identity-mapped 4K
    let mut table = PML4::new();
    table
        .create_mapping(0, 0x1000, 0, PageAttr::ReadWriteKernel)
        .expect("Failed to create mapping");
    assert_eq!(table.translate(0x0000), Ok(PageMapped4K { phys: 0x0000 }));
    assert_eq!(table.translate(0x1000), Err(Error::PageNotFound));
    // Non-identity-mapped 4K
    let mut table = PML4::new();
    table
        .create_mapping(0, 0x1000, 0x1000, PageAttr::ReadWriteKernel)
        .expect("Failed to create mapping");
    assert_eq!(table.translate(0x0000), Ok(PageMapped4K { phys: 0x1000 }));
    assert_eq!(table.translate(0x1000), Err(Error::PageNotFound));
}
