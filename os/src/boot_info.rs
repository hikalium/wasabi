use crate::acpi::Acpi;
use crate::arch::x86_64::apic::LocalApic;
use crate::arch::x86_64::read_cpuid;
use crate::arch::x86_64::CpuidRequest;
use crate::arch::x86_64::CpuidResponse;
use crate::efi::EfiFileName;
use crate::error::Error;
use crate::memory_map_holder::MemoryMapHolder;
use crate::vram::VRAMBufferInfo;
use core::fmt;
use core::mem::size_of_val;
use core::slice;
use core::str;

pub struct File {
    name: EfiFileName,
    data: &'static mut [u8],
}
impl File {
    /// # Safety
    ///
    /// passed data and len should be valid
    pub unsafe fn from_raw(name: EfiFileName, data: *mut u8, len: usize) -> Result<Self, Error> {
        Ok(Self {
            name,
            data: core::slice::from_raw_parts_mut(data, len),
        })
    }
    pub fn name(&self) -> &EfiFileName {
        &self.name
    }
    pub fn has_name(&self, name: &str) -> bool {
        name.encode_utf16()
            .zip(self.name.name().iter())
            .all(|(l, &r)| l == r)
    }
    pub fn data(&self) -> &[u8] {
        self.data
    }
}

#[derive(Clone)]
pub struct FixedString<const N: usize> {
    values: [u32; N],
}
impl<const N: usize> FixedString<{ N }> {
    fn new(values: &[u32; N]) -> Self {
        Self { values: *values }
    }
}
impl<const N: usize> fmt::Debug for FixedString<{ N }> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let slice = unsafe {
            slice::from_raw_parts(self.values.as_ptr() as *const u8, size_of_val(&self.values))
        };
        let s = str::from_utf8(slice).expect("failed to parse utf8 str");
        write!(f, "{s}")
    }
}

pub struct ModelFamilyStepping {
    value: u32,
}
impl ModelFamilyStepping {
    fn new(value: u32) -> Self {
        Self { value }
    }
    fn family(&self) -> u32 {
        let family = (self.value >> 8) & 0b1111;
        if family != 0x0F {
            family
        } else {
            let extended_family = (self.value >> 20) & 0b1111_1111;
            extended_family + family
        }
    }
    fn model(&self) -> u32 {
        let model = (self.value >> 4) & 0b1111;
        if model == 0x06 || model == 0x0F {
            let extended_model = (self.value >> 16) & 0b1111;
            (extended_model << 4) + model
        } else {
            model
        }
    }
    fn stepping(&self) -> u32 {
        self.value & 0b1111
    }
}
impl fmt::Debug for ModelFamilyStepping {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:#010X} (family: {:#04X}, model: {:#04X}, stepping: {:#03X})",
            self.value,
            self.family(),
            self.model(),
            self.stepping()
        )
    }
}

#[derive(Debug)]
pub struct CpuFeatures {
    pub max_basic_cpuid: u32,
    pub max_extended_cpuid: u32,
    pub has_x2apic: bool,
    pub has_tsc_deadline_mode: bool,
    pub has_invariant_tsc: bool,
    pub vendor_string: FixedString<3>,
    pub cpu_brand_string: Option<FixedString<12>>,
    pub model_family_stepping: ModelFamilyStepping,
}
impl CpuFeatures {
    fn inspect() -> Self {
        let query = |q: CpuidRequest| -> CpuidResponse {
            let r = read_cpuid(q);
            crate::println!("{} -> {}", q, r);
            r
        };
        let c = query(CpuidRequest { eax: 0, ecx: 0 });
        let max_basic_cpuid = c.eax();
        let vendor_string = FixedString::<3>::new(&[c.ebx(), c.edx(), c.ecx()]);
        crate::println!("cpuid(0, 0).vendor = {:?}", &vendor_string);

        let leaf01 = query(CpuidRequest { eax: 1, ecx: 0 });

        let max_extended_cpuid = query(CpuidRequest {
            eax: 0x8000_0000,
            ecx: 0,
        })
        .eax();
        let has_invariant_tsc = if max_extended_cpuid >= 0x8000_0007 {
            query(CpuidRequest {
                eax: 0x8000_0007,
                ecx: 0,
            })
            .edx()
                & (1 << 8)
                != 0
        } else {
            false
        };

        let cpu_brand_string = if max_extended_cpuid >= 0x8000_0004 {
            let c2 = query(CpuidRequest {
                eax: 0x8000_0002,
                ecx: 0,
            });
            let c3 = query(CpuidRequest {
                eax: 0x8000_0003,
                ecx: 0,
            });
            let c4 = query(CpuidRequest {
                eax: 0x8000_0004,
                ecx: 0,
            });
            let cpu_brand_string = FixedString::<12>::new(&[
                c2.eax(),
                c2.ebx(),
                c2.ecx(),
                c2.edx(),
                c3.eax(),
                c3.ebx(),
                c3.ecx(),
                c3.edx(),
                c4.eax(),
                c4.ebx(),
                c4.ecx(),
                c4.edx(),
            ]);
            crate::println!("cpu_brand_string = {:?}", cpu_brand_string);
            Some(cpu_brand_string)
        } else {
            None
        };

        CpuFeatures {
            max_basic_cpuid,
            max_extended_cpuid,
            has_x2apic: ((leaf01.ecx() >> 21) & 1) != 0,
            has_tsc_deadline_mode: ((leaf01.ecx() >> 24) & 1) != 0,
            has_invariant_tsc,
            vendor_string,
            cpu_brand_string,
            model_family_stepping: ModelFamilyStepping::new(leaf01.eax()),
        }
    }
}

pub struct BootInfo {
    vram: VRAMBufferInfo,
    memory_map: MemoryMapHolder,
    root_files: [Option<File>; 32],
    acpi: Acpi,
    cpu_features: CpuFeatures,
    bsp_local_apic: LocalApic,
    kernel_stack: &'static [u8],
}
impl BootInfo {
    pub fn new(
        vram: VRAMBufferInfo,
        memory_map: MemoryMapHolder,
        root_files: [Option<File>; 32],
        acpi: Acpi,
        kernel_stack: &'static [u8],
    ) -> BootInfo {
        let cpu_features = CpuFeatures::inspect();
        let bsp_local_apic = LocalApic::default();
        BootInfo {
            vram,
            memory_map,
            root_files,
            acpi,
            cpu_features,
            bsp_local_apic,
            kernel_stack,
        }
    }
    pub fn vram(&self) -> VRAMBufferInfo {
        self.vram
    }
    pub fn memory_map(&'static self) -> &'static MemoryMapHolder {
        &self.memory_map
    }
    pub fn root_files(&self) -> &[Option<File>; 32] {
        &self.root_files
    }
    pub fn acpi(&self) -> &Acpi {
        &self.acpi
    }
    pub fn cpu_features(&self) -> &CpuFeatures {
        &self.cpu_features
    }
    pub fn bsp_local_apic(&self) -> &LocalApic {
        &self.bsp_local_apic
    }
    pub fn kernel_stack(&self) -> &'static [u8] {
        self.kernel_stack
    }
    pub fn take() -> &'static BootInfo {
        // SAFETY: Taking static immutable reference here is
        // safe because BOOT_INFO is only set once and
        // no one will take a mutable reference to it.
        unsafe { BOOT_INFO.as_ref().expect("BOOT_INFO not initialized yet") }
    }
    /// # Safety
    ///
    /// This function panics when it is called twice, to ensure that Some(boot_info) has a "static"
    /// lifetime
    pub unsafe fn set(boot_info: BootInfo) {
        assert!(BOOT_INFO.is_none());
        crate::println!("BOOT_INFO populated!");
        BOOT_INFO = Some(boot_info);
    }
}
unsafe impl Sync for BootInfo {
    // This Sync impl is fake
    // but read access to it will be safe
}
static mut BOOT_INFO: Option<BootInfo> = None;
