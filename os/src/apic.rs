use crate::error::Result;
use crate::error::WasabiError;
use crate::println;
use crate::x86;
use crate::x86::CpuidRequest;
use crate::BootInfo;

#[derive(Debug)]
#[allow(dead_code)]
pub struct LocalApic {
    x2apic_id: u32,
    status: LocalApicStatus,
    base_addr: u64,
}

impl LocalApic {
    /// creates an instance to manage Local APIC for the current processor
    pub fn new() -> Self {
        let cpu_features = BootInfo::take().cpu_features();
        println!("{:?}", cpu_features);
        let x2apic_id = x86::read_cpuid(CpuidRequest { eax: 0x0b, ecx: 0 }).edx();
        println!("x2APIC ID: {}", x2apic_id);
        let apic_base = x86::read_msr(x86::MSR_IA32_APIC_BASE);
        println!("MSR_IA32_APIC_BASE={:#X}", apic_base);
        let status = LocalApicStatus::new(apic_base);
        println!("{:?}", status);
        Self {
            x2apic_id,
            base_addr: apic_base & ((1u64 << 12) - 1),
            status,
        }
    }
    fn id(&self) -> u32 {
        self.x2apic_id
    }
}

impl Default for LocalApic {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct LocalApicStatus {
    is_bsp: bool,
    x2apic_mode_enable: bool,
    apic_global_enable: bool,
}

impl LocalApicStatus {
    fn new(apic_base: u64) -> Self {
        Self {
            is_bsp: apic_base & (1u64 << 8) != 0,
            x2apic_mode_enable: apic_base & (1u64 << 10) != 0,
            apic_global_enable: apic_base & (1u64 << 11) != 0,
        }
    }
}

const IOAPIC_INDEX_ADDR: *mut u8 = 0xfec00000 as *mut u8;
const IOAPIC_DATA_ADDR: *mut u32 = 0xfec00010 as *mut u32;

pub struct IoApic {}
impl IoApic {
    fn read_register(index: usize) -> Result<u32> {
        if index >= 0x100 {
            Err(WasabiError::ApicRegIndexOutOfRange)
        } else {
            // This is safe since the index is checked
            unsafe {
                IOAPIC_INDEX_ADDR.write_volatile(index as u8);
                Ok(IOAPIC_DATA_ADDR.read_volatile())
            }
        }
    }
    fn write_register(index: usize, data: u32) -> Result<()> {
        if index >= 0x100 {
            Err(WasabiError::ApicRegIndexOutOfRange)
        } else {
            // This is safe since the index is checked
            unsafe {
                IOAPIC_INDEX_ADDR.write_volatile(index as u8);
                IOAPIC_DATA_ADDR.write_volatile(data);
            }
            Ok(())
        }
    }
    fn read_redirection_entry(irq: usize) -> Result<u64> {
        if irq < 24 {
            let v = (
                Self::read_register(0x10 + irq * 2),
                Self::read_register(0x10 + irq * 2 + 1),
            );
            if let (Ok(low), Ok(high)) = v {
                return Ok((low as u64) | (high as u64) << 32);
            }
        }
        Err(WasabiError::ApicRegIndexOutOfRange)
    }
    fn write_redirection_entry(irq: usize, entry: u64) -> Result<()> {
        Self::write_register(0x10 + irq * 2, entry as u32)?;
        Self::write_register(0x10 + irq * 2 + 1, (entry >> 32) as u32)?;
        Ok(())
    }
    fn set_redirection(from_irq: usize, to_vector: u8, to_apic: u32) -> Result<()> {
        let entry: u64 = ((to_apic as u64) << 56) | (to_vector as u64);
        Self::write_redirection_entry(from_irq, entry)
    }
    pub fn init(bsp_lapic: &LocalApic) {
        println!("Initial redirection:");
        for i in 0..24 {
            println!(
                "{:#04X}: {:#018X}",
                i,
                Self::read_redirection_entry(i).expect("Failed to read redirection table")
            );
        }

        let to_apic_id = bsp_lapic.id();
        Self::set_redirection(2, 0x20, to_apic_id); // HPET
        Self::set_redirection(1, 0x21, to_apic_id); // KBC
        Self::set_redirection(12, 0x22, to_apic_id); // Mouse

        println!("After redirection:");
        for i in 0..24 {
            println!(
                "{:#04X}: {:#018X}",
                i,
                Self::read_redirection_entry(i).expect("Failed to read redirection table")
            );
        }
    }
}
