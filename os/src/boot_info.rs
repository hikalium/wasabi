use crate::acpi::Acpi;
use crate::arch::x86_64::read_cpuid;
use crate::arch::x86_64::CpuidRequest;
use crate::arch::x86_64::CpuidResponse;
use crate::efi::EfiFileName;
use crate::error::WasabiError;
use crate::MemoryMapHolder;
use crate::VRAMBufferInfo;
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
    pub unsafe fn from_raw(
        name: EfiFileName,
        data: *mut u8,
        len: usize,
    ) -> Result<Self, WasabiError> {
        Ok(Self {
            name,
            data: core::slice::from_raw_parts_mut(data, len),
        })
    }
    pub fn name(&self) -> &EfiFileName {
        &self.name
    }
    pub fn data(&self) -> &[u8] {
        self.data
    }
}

#[derive(Debug)]
pub struct CpuFeatures {
    pub max_basic_cpuid: u32,
    pub has_x2apic: bool,
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
        let slice = unsafe { slice::from_raw_parts(&c as *const CpuidResponse as *const u8, 12) };
        let vendor = str::from_utf8(slice).expect("failed to parse utf8 str");
        crate::println!("cpuid(0, 0).vendor = {}", vendor);

        let eax01 = query(CpuidRequest { eax: 1, ecx: 0 });

        CpuFeatures {
            max_basic_cpuid,
            has_x2apic: (eax01.ecx() >> 21) != 0,
        }
    }
}

pub struct BootInfo {
    vram: VRAMBufferInfo,
    memory_map: MemoryMapHolder,
    root_files: [Option<File>; 32],
    acpi: Acpi,
    cpu_features: CpuFeatures,
}
impl BootInfo {
    pub fn new(
        vram: VRAMBufferInfo,
        memory_map: MemoryMapHolder,
        root_files: [Option<File>; 32],
        acpi: Acpi,
    ) -> BootInfo {
        BootInfo {
            vram,
            memory_map,
            root_files,
            acpi,
            cpu_features: CpuFeatures::inspect(),
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
    /// # Safety
    ///
    /// Taking static immutable reference here is safe because BOOT_INFO is only set once and no
    /// one will take a mutable reference to it.
    pub fn take() -> &'static BootInfo {
        unsafe { BOOT_INFO.as_ref().expect("BOOT_INFO not initialized yet") }
    }
    /// # Safety
    ///
    /// This function panics when it is called twice, to ensure that Some(boot_info) has a "static"
    /// lifetime
    pub unsafe fn set(boot_info: BootInfo) {
        assert!(BOOT_INFO.is_none());
        BOOT_INFO = Some(boot_info);
    }
}
unsafe impl Sync for BootInfo {
    // This Sync impl is fake
    // but read access to it will be safe
}
static mut BOOT_INFO: Option<BootInfo> = None;
