use crate::println;
use crate::x86;

#[derive(Debug)]
pub struct LocalApic {
    base_addr: u64,
}

impl LocalApic {
    /// creates an instance to manage Local APIC for the current processor
    pub fn new() -> Self {
        let apic_base = x86::read_msr(x86::MSR_IA32_APIC_BASE);
        println!("MSR_IA32_APIC_BASE={:#X}", apic_base);
        let apic_status = LocalApicStatus::new(apic_base);
        println!("{:?}", apic_status);
        Self {
            base_addr: apic_base & ((1u64 << 12) - 1),
        }
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
