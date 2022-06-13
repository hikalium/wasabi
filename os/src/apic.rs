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
