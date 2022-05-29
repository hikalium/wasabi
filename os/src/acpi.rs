use crate::error::*;
use crate::*;

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
struct Xsdt {
    signature: [u8; 4],
    length: u32,
    _unused: [u8; 28],
}

impl Xsdt {
    fn list_all_tables(&self) {
        assert_eq!(&self.signature, b"XSDT");
        let xsdt_fixed_size = core::mem::size_of::<Self>();
        let num_of_entries =
            (self.length as usize - xsdt_fixed_size) / core::mem::size_of::<*const u8>();
        println!("ACPI tables in XSDT:");
        unsafe {
            let entries =
                (self as *const Self as *const u8).add(xsdt_fixed_size) as *const *const u8;
            for i in 0..num_of_entries {
                let signature =
                    core::str::from_utf8_unchecked(core::slice::from_raw_parts(*entries.add(i), 4));
                println!("XSDT[{}]: {}", i, signature);
            }
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
        Ok(Acpi {})
    }
}
