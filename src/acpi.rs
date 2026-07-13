use crate::hpet::HpetRegisters;
use crate::result::Result;
use core::fmt;
use core::mem::size_of;

#[repr(packed)]
#[derive(Clone, Copy, Debug)]
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
                &*(self.table.entry(self.index - 1)
                    as *const SystemDescriptionTableHeader)
            })
        }
    }
}

#[repr(packed)]
struct Xsdt {
    header: SystemDescriptionTableHeader,
}
const _: () = assert!(size_of::<Xsdt>() == 36);

impl Xsdt {
    fn find_table(
        &self,
        sig: &'static [u8; 4],
    ) -> Option<&'static SystemDescriptionTableHeader> {
        self.iter().find(|&e| e.signature() == sig)
    }
    fn header_size(&self) -> usize {
        size_of::<Self>()
    }
    fn num_of_entries(&self) -> usize {
        (self.header.length as usize - self.header_size())
            / size_of::<*const u8>()
    }
    unsafe fn entry(&self, index: usize) -> *const u8 {
        ((self as *const Self as *const u8).add(self.header_size())
            as *const *const u8)
            .add(index)
            .read_unaligned()
    }
    fn iter(&self) -> XsdtIterator {
        XsdtIterator::new(self)
    }
}

trait AcpiTable {
    const SIGNATURE: &'static [u8; 4];
    type Table;
    fn new(header: &SystemDescriptionTableHeader) -> &Self::Table {
        header.expect_signature(Self::SIGNATURE);
        // This is safe as far as phys_addr points to a valid MCFG table and it
        // alives forever.
        let mcfg: &Self::Table = unsafe {
            &*(header as *const SystemDescriptionTableHeader
                as *const Self::Table)
        };
        mcfg
    }
}

#[derive(Copy, Clone)]
#[repr(packed)]
pub struct GenericAddress {
    address_space_id: u8,
    _unused: [u8; 3],
    address: u64,
}
const _: () = assert!(size_of::<GenericAddress>() == 12);
impl GenericAddress {
    pub fn is_in_memory_space(&self) -> bool {
        self.address_space_id == 0x00
    }
    pub fn is_in_register_space(&self) -> bool {
        self.address_space_id == 0x01
    }
    pub fn address_in_memory_space(&self) -> Result<usize> {
        if self.address_space_id == 0 {
            Ok(self.address as usize)
        } else {
            Err("ACPI Generic Address is not in system memory space")
        }
    }
    pub fn address_in_io_space(&self) -> Result<u16> {
        if self.address_space_id == 1 {
            self.address
                .try_into()
                .or(Err("Address in IO space outside of 16bit range"))
        } else {
            Err("ACPI Generic Address is not in system memory space")
        }
    }
}

#[repr(packed)]
pub struct AcpiHpetDescriptor {
    _header: SystemDescriptionTableHeader,
    _reserved0: u32,
    address: GenericAddress,
    _reserved1: u32,
}
impl AcpiTable for AcpiHpetDescriptor {
    const SIGNATURE: &'static [u8; 4] = b"HPET";
    type Table = Self;
}
impl AcpiHpetDescriptor {
    pub fn base_address(&self) -> Result<&'static mut HpetRegisters> {
        unsafe {
            self.address
                .address_in_memory_space()
                .map(|addr| &mut *(addr as *mut HpetRegisters))
        }
    }
}
const _: () = assert!(size_of::<AcpiHpetDescriptor>() == 56);

#[repr(C)]
#[derive(Debug)]
pub struct AcpiRsdpStruct {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt: u64,
}
impl AcpiRsdpStruct {
    fn xsdt(&self) -> &Xsdt {
        unsafe { &*(self.xsdt as *const Xsdt) }
    }
    pub fn hpet(&self) -> Option<&AcpiHpetDescriptor> {
        let xsdt = self.xsdt();
        xsdt.find_table(b"HPET").map(AcpiHpetDescriptor::new)
    }
    pub fn mcfg(&self) -> Option<&AcpiMcfgDescriptor> {
        let xsdt = self.xsdt();
        xsdt.find_table(b"MCFG").map(AcpiMcfgDescriptor::new)
    }
    pub fn fadt(&self) -> Option<&AcpiFadtDescriptor> {
        let xsdt = self.xsdt();
        xsdt.find_table(b"FACP").map(AcpiFadtDescriptor::new)
    }
}

#[repr(C, packed)]
#[derive(Debug)]
#[allow(dead_code)]
pub struct AcpiMcfgDescriptor {
    // https://wiki.osdev.org/PCI_Express
    header: SystemDescriptionTableHeader,
    _unused: [u8; 8],
    // 44 + (16 * n) -> Configuration space base address allocation structures
    // [EcamEntry; ?]
}
impl AcpiTable for AcpiMcfgDescriptor {
    const SIGNATURE: &'static [u8; 4] = b"MCFG";
    type Table = Self;
}
const _: () = assert!(size_of::<AcpiMcfgDescriptor>() == 44);
impl AcpiMcfgDescriptor {
    pub fn header_size(&self) -> usize {
        size_of::<Self>()
    }
    pub fn num_of_entries(&self) -> usize {
        (self.header.length as usize - self.header_size())
            / size_of::<EcamEntry>()
    }
    pub fn entry(&self, index: usize) -> Option<&EcamEntry> {
        if index >= self.num_of_entries() {
            None
        } else {
            Some(unsafe {
                &*((self as *const Self as *const u8).add(self.header_size())
                    as *const EcamEntry)
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
            "ECAM: Bus [{}..={}] is mapped at {:#X}",
            bus_start, bus_end, base
        )
    }
}

#[derive(Debug, Clone)]
pub enum RebootParams {
    Memory { addr: usize, value: u8 },
    Io { addr: u16, value: u8 },
}
#[repr(C, packed)]
#[derive(Debug)]
#[allow(dead_code)]
pub struct AcpiFadtDescriptor {
    header: SystemDescriptionTableHeader,
    firmware_ctrl: u32,
    dsdt: u32,
    reserved: u8,
    preferred_pm_profile: u8,
    sci_int: u16,
    smi_cmd: u32,
    acpi_enable: u8,
    acpi_disable: u8,
}
impl AcpiTable for AcpiFadtDescriptor {
    const SIGNATURE: &'static [u8; 4] = b"FACP";
    type Table = Self;
}
const _: () = assert!(size_of::<AcpiFadtDescriptor>() == 54);
impl AcpiFadtDescriptor {
    fn reboot_address(&self) -> Result<GenericAddress> {
        const OFFSET: isize = 116;
        if self.header.length as isize >= OFFSET {
            unsafe {
                Ok(*(self as *const Self as *const GenericAddress)
                    .byte_offset(OFFSET))
            }
        } else {
            Err("Failed to get the reset register address")
        }
    }
    fn reboot_value(&self) -> Result<u8> {
        const OFFSET: isize = 128;
        if self.header.length as isize >= OFFSET {
            Ok(unsafe {
                *(self as *const Self as *const u8).byte_offset(OFFSET)
            })
        } else {
            Err("Failed to get the reset register address")
        }
    }
    pub fn reset_params(&self) -> Result<RebootParams> {
        let addr = self.reboot_address()?;
        let value = self.reboot_value()?;
        if let Ok(addr) = addr.address_in_memory_space() {
            Ok(RebootParams::Memory { addr, value })
        } else if let Ok(addr) = addr.address_in_io_space() {
            Ok(RebootParams::Io { addr, value })
        } else {
            Err("Unsupported Generic Address type")
        }
    }
}
