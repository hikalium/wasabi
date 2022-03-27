use crate::*;

pub struct BootInfo {
    vram: VRAMBufferInfo,
    memory_map: MemoryMapHolder,
}
impl BootInfo {
    pub fn new(vram: VRAMBufferInfo, memory_map: MemoryMapHolder) -> BootInfo {
        BootInfo { vram, memory_map }
    }
    pub fn vram(&self) -> VRAMBufferInfo {
        self.vram
    }
    pub fn memory_map(&'static self) -> &'static MemoryMapHolder {
        &self.memory_map
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
