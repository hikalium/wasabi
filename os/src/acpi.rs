use crate::error::Error;
use crate::error::Result;
use crate::hpet;
use crate::info;
use core::fmt;
use core::mem::size_of;
use core::slice;

#[repr(C)]
#[derive(Debug)]
pub struct RsdpStruct {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt: u64,
}

impl RsdpStruct {
    fn xsdt(&self) -> &Xsdt {
        unsafe { &*(self.xsdt as *const Xsdt) }
    }
}

#[repr(packed)]
#[derive(Clone, Copy)]
struct SystemDescriptionTableHeader {
    // 5.2. ACPI System Description Tables
    // Table 5.4: DESCRIPTION_HEADER Fields
    signature: [u8; 4],
    length: u32,
    _unused: [u8; 28],
}
const _: () = assert!(size_of::<SystemDescriptionTableHeader>() == 36);

impl SystemDescriptionTableHeader {
    fn expect_signature(&self, sig: &'static [u8; 4]) {
        assert_eq!(self.signature, *sig);
    }
    fn signature(&self) -> &[u8; 4] {
        &self.signature
    }
    fn signature_string(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(slice::from_raw_parts(self.signature.as_ptr(), 4)) }
    }
}

impl fmt::Debug for SystemDescriptionTableHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // To avoid "error: reference to packed field is unaligned"
        let length = self.length;
        f.debug_struct("Header")
            .field("signature", &self.signature_string())
            .field("length", &length)
            .finish()
    }
}

trait AcpiTable {
    const SIGNATURE: &'static [u8; 4];
    type Table;
    fn table(&self) -> &Self::Table;
    fn new(header: &SystemDescriptionTableHeader) -> &Self::Table {
        header.expect_signature(Self::SIGNATURE);
        // This is safe as far as phys_addr points to a valid MCFG table and it alives forever.
        let mcfg: &Self::Table =
            unsafe { &*(header as *const SystemDescriptionTableHeader as *const Self::Table) };
        mcfg
    }
}

#[repr(packed)]
#[derive(Debug)]
#[allow(dead_code)]
pub struct Mcfg {
    // https://wiki.osdev.org/PCI_Express
    header: SystemDescriptionTableHeader,
    _unused: [u8; 8],
    // 44 + (16 * n) -> Configuration space base address allocation structures
    // [EcamEntry; ?]
}
impl AcpiTable for Mcfg {
    const SIGNATURE: &'static [u8; 4] = b"MCFG";
    type Table = Self;
    fn table(&self) -> &Self::Table {
        self
    }
}
const _: () = assert!(size_of::<Mcfg>() == 44);
impl Mcfg {
    pub fn header_size(&self) -> usize {
        size_of::<Self>()
    }
    pub fn num_of_entries(&self) -> usize {
        (self.header.length as usize - self.header_size()) / size_of::<EcamEntry>()
    }
    pub fn entry(&self, index: usize) -> Option<&EcamEntry> {
        if index >= self.num_of_entries() {
            None
        } else {
            Some(unsafe {
                &*((self as *const Self as *const u8).add(self.header_size()) as *const EcamEntry)
                    .add(index)
            })
        }
    }
}

#[repr(packed)]
pub struct EcamEntry {
    ecm_base_addr: u64,
    _pci_segment_group: u16,
    start_pci_bus: u8,
    end_pci_bus: u8,
    _reserved: u32,
}
impl EcamEntry {
    pub fn base_address(&self) -> u64 {
        self.ecm_base_addr
    }
}
impl fmt::Display for EcamEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // To avoid "error: reference to packed field is unaligned"
        let base = self.ecm_base_addr;
        let bus_start = self.start_pci_bus;
        let bus_end = self.end_pci_bus;
        write!(
            f,
            "ECAM @ {:#p}: Bus range [{},{}] is mapped at {:#X}",
            self, bus_start, bus_end, base
        )
    }
}

trait AcpiIterableTable {
    type Item;
}

#[repr(packed)]
struct Xsdt {
    header: SystemDescriptionTableHeader,
}
const _: () = assert!(size_of::<Xsdt>() == 36);

impl Xsdt {
    fn list_all_tables(&self) {
        self.header.expect_signature(b"XSDT");
        info!("ACPI tables in XSDT:");
        for (i, e) in self.iter().enumerate() {
            info!("XSDT[{}]: {:?}", i, e);
        }
    }
    fn find_table(&self, sig: &'static [u8; 4]) -> Option<&'static SystemDescriptionTableHeader> {
        self.iter().find(|&e| e.signature() == sig)
    }
    fn header_size(&self) -> usize {
        size_of::<Self>()
    }
    fn num_of_entries(&self) -> usize {
        (self.header.length as usize - self.header_size()) / size_of::<*const u8>()
    }
    unsafe fn entry(&self, index: usize) -> *const u8 {
        ((self as *const Self as *const u8).add(self.header_size()) as *const *const u8)
            .add(index)
            .read_unaligned()
    }
    fn iter(&self) -> XsdtIterator {
        XsdtIterator::new(self)
    }
}

struct XsdtIterator<'a> {
    table: &'a Xsdt,
    index: usize,
}

impl<'a> XsdtIterator<'a> {
    pub fn new(table: &'a Xsdt) -> Self {
        XsdtIterator { table, index: 0 }
    }
}
impl<'a> Iterator for XsdtIterator<'a> {
    // The item will have a static lifetime
    // since it will be allocated on
    // ACPI_RECLAIM_MEMORY region.
    type Item = &'static SystemDescriptionTableHeader;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.table.num_of_entries() {
            None
        } else {
            self.index += 1;
            Some(unsafe {
                &*(self.table.entry(self.index - 1) as *const SystemDescriptionTableHeader)
            })
        }
    }
}

#[repr(packed)]
pub struct GenericAddress {
    address_space_id: u8,
    _unused: [u8; 3],
    address: u64,
}
const _: () = assert!(size_of::<GenericAddress>() == 12);
impl GenericAddress {
    pub fn address_in_memory_space(&self) -> Result<usize> {
        if self.address_space_id == 0 {
            Ok(self.address as usize)
        } else {
            Err(Error::Failed(
                "ACPI Generic Address is not in system memory space",
            ))
        }
    }
}

#[repr(packed)]
pub struct Hpet {
    _header: SystemDescriptionTableHeader,
    _reserved0: u32,
    address: GenericAddress,
    _reserved1: u32,
}
impl AcpiTable for Hpet {
    const SIGNATURE: &'static [u8; 4] = b"HPET";
    type Table = Self;
    fn table(&self) -> &Self::Table {
        self
    }
}
impl Hpet {
    pub fn base_address(&self) -> Result<&mut hpet::Registers> {
        unsafe {
            self.address
                .address_in_memory_space()
                .map(|addr| &mut *(addr as *mut hpet::Registers))
        }
    }
}
const _: () = assert!(size_of::<Hpet>() == 56);

#[repr(packed)]
pub struct Dsdt {
    header: SystemDescriptionTableHeader,
}
impl Dsdt {
    pub fn term_list_slice(&self) -> &[u8] {
        let header_size = size_of::<Self>();
        let term_list_size = self.header.length as usize - header_size;
        unsafe {
            slice::from_raw_parts(
                (self as *const Dsdt as *const u8).add(header_size),
                term_list_size,
            )
        }
    }
}
impl AcpiTable for Dsdt {
    const SIGNATURE: &'static [u8; 4] = b"DSDT";
    type Table = Self;
    fn table(&self) -> &Self::Table {
        self
    }
}
impl fmt::Debug for Dsdt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Dsdt {{ {:?} }}", self.header)
    }
}

#[repr(packed)]
pub struct Fadt {
    _header: SystemDescriptionTableHeader,
    _reserved0: u32,
    dsdt: u32,
}
impl AcpiTable for Fadt {
    const SIGNATURE: &'static [u8; 4] = b"FACP";
    type Table = Self;
    fn table(&self) -> &Self::Table {
        self
    }
}
impl Fadt {
    pub fn dsdt(&self) -> &Dsdt {
        let header = self.dsdt as usize as *const SystemDescriptionTableHeader;
        Dsdt::new(unsafe { &*header })
    }
}

pub struct Acpi {
    mcfg: &'static Mcfg,
    hpet: &'static Hpet,
    dsdt: &'static Dsdt,
}
impl<'a> Acpi {
    pub fn new(rsdp_struct: &RsdpStruct) -> Result<Acpi> {
        if &rsdp_struct.signature != b"RSD PTR " {
            return Err("Invalid RSDP Struct Signature".into());
        }
        if rsdp_struct.revision < 2 {
            return Err("Expected RSDP rev.2 or above".into());
        }
        let xsdt = rsdp_struct.xsdt();
        xsdt.list_all_tables();

        let mcfg = Mcfg::new(xsdt.find_table(b"MCFG").expect("MCFG not found"));
        let hpet = Hpet::new(xsdt.find_table(b"HPET").expect("HPET not found"));
        let fadt = Fadt::new(xsdt.find_table(b"FACP").expect("FACP not found"));
        let dsdt = fadt.dsdt();
        Ok(Acpi { mcfg, hpet, dsdt })
    }
    pub fn dsdt(&'a self) -> &'a Dsdt {
        self.dsdt
    }
    pub fn hpet(&'a self) -> &'a Hpet {
        self.hpet
    }
    pub fn mcfg(&'a self) -> &'a Mcfg {
        self.mcfg
    }
}
