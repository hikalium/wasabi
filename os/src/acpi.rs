use crate::error::*;
use crate::*;
use core::fmt;
use core::mem::size_of;

#[repr(C)]
#[derive(Debug)]
struct RsdpStruct {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt: u64,
}

#[repr(packed)]
struct SystemDescriptionTableHeader {
    // acpi_6_4.pdf#page=166
    // 32 bytes
    signature: [u8; 4],
    length: u32,
    _unused: [u8; 24],
}

impl SystemDescriptionTableHeader {
    fn expect_signature(&self, sig: &'static [u8; 4]) {
        assert_eq!(self.signature, *sig);
    }
    fn signature(&self) -> &[u8; 4] {
        &self.signature
    }
    fn signature_string(&self) -> &str {
        unsafe {
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(self.signature.as_ptr(), 4))
        }
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

#[repr(packed)]
#[derive(Debug)]
#[allow(dead_code)]
struct Mcfg {
    // https://wiki.osdev.org/PCI_Express
    header: SystemDescriptionTableHeader,
    _unused: [u8; 12],
    // 44 + (16 * n) -> Configuration space base address allocation structures
    // [EcamEntry; ?]
}

impl Mcfg {
    pub fn new(header: &SystemDescriptionTableHeader) -> &Self {
        header.expect_signature(b"MCFG");
        println!("Got valid MCFG");
        // This is safe as far as phys_addr points to a valid MCFG table and it alives forever.
        let mcfg: &Self = unsafe { core::mem::transmute(header) };
        mcfg
    }
}

#[derive(Debug)]
#[repr(C)]
struct EcamEntry {
    ecm_base_phys_addr: u64,
    pci_segment_group: u16,
    start_pci_bus: u8,
    end_pci_bus: u8,
    _reserved: u32,
}

trait AcpiIterableTable {
    type Item;
}

#[repr(packed)]
struct Xsdt {
    header: SystemDescriptionTableHeader,
    _unused: [u8; 4],
}

impl Xsdt {
    fn list_all_tables(&self) {
        self.header.expect_signature(b"XSDT");
        println!("ACPI tables in XSDT:");
        for (i, e) in self.iter().enumerate() {
            println!("XSDT[{}]: {:?}", i, e);
        }
    }
    fn find_table(&self, sig: &'static [u8; 4]) -> Option<&SystemDescriptionTableHeader> {
        for e in self.iter() {
            if e.signature() == sig {
                println!("{} Found!", e.signature_string());
                return Some(e);
            }
        }
        None
    }
    fn header_size(&self) -> usize {
        size_of::<Self>()
    }
    fn num_of_entries(&self) -> usize {
        (self.header.length as usize - self.header_size()) / core::mem::size_of::<*const u8>()
    }
    unsafe fn entry(&self, index: usize) -> *const u8 {
        *((self as *const Self as *const u8).add(self.header_size()) as *const *const u8).add(index)
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
    type Item = &'a SystemDescriptionTableHeader;
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

pub struct Acpi {}
impl Acpi {
    pub fn new(rsdp: *const u8) -> Result<Acpi> {
        let rsdp_struct = unsafe {
            let rsdp_struct = &*(rsdp as *const RsdpStruct);
            if &rsdp_struct.signature != b"RSD PTR " {
                return Err("Invalid RSDP Struct Signature".into());
            }
            rsdp_struct
        };
        println!("{:?}", rsdp_struct);
        if rsdp_struct.revision < 2 {
            return Err("Expected RSDP rev.2 or above".into());
        }
        let xsdt = unsafe { &*(rsdp_struct.xsdt as *const Xsdt) };
        xsdt.list_all_tables();

        let mcfg = Mcfg::new(xsdt.find_table(b"MCFG").expect("MCFG not found"));
        println!("{:?}", mcfg);
        Ok(Acpi {})
    }
}
