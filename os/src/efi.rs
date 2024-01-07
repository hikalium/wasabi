pub mod constants;
pub mod fs;
pub mod types;

use crate::error::Error;
use crate::error::Result;
use crate::memory_map_holder;
use crate::memory_map_holder::MemoryMapHolder;
use crate::util::size_in_pages_from_bytes;
use core::fmt;
use core::marker::PhantomPinned;
use core::mem::size_of;
use core::pin::Pin;
use core::ptr::null_mut;

use core::mem::offset_of;
use fs::EfiSimpleFileSystemProtocol;
use types::EfiGuid;
use types::EfiHandle;
use types::EfiStatus;
use types::EfiTableHeader;
use types::EfiVoid;

#[repr(C)]
pub struct EfiSimpleTextOutputProtocol {
    reset: EfiHandle,
    output_string:
        extern "win64" fn(this: *const EfiSimpleTextOutputProtocol, str: *const u16) -> EfiStatus,
    test_string: EfiHandle,
    query_mode: EfiHandle,
    set_mode: EfiHandle,
    set_attribute: EfiHandle,
    clear_screen: extern "win64" fn(this: *const EfiSimpleTextOutputProtocol) -> EfiStatus,
    //
    _pinned: PhantomPinned,
}

impl EfiSimpleTextOutputProtocol {
    pub fn clear_screen(&self) -> Result<()> {
        (self.clear_screen)(self).into_result()
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct EfiLoadedImageProtocol {
    _padding0: [u64; 3],
    pub device_handle: EfiHandle,
    _pinned: PhantomPinned,
}
const _: () = assert!(offset_of!(EfiLoadedImageProtocol, device_handle) == 24);

#[repr(C)]
#[derive(Debug)]
pub struct EfiGraphicsOutputProtocolPixelInfo {
    version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    _padding0: [u32; 5],
    pub pixels_per_scan_line: u32,
    _pinned: PhantomPinned,
}
const _: () = assert!(size_of::<EfiGraphicsOutputProtocolPixelInfo>() == 36);

#[repr(C)]
#[derive(Debug)]
pub struct EfiGraphicsOutputProtocolMode<'a> {
    pub max_mode: u32,
    pub mode: u32,
    pub info: &'a EfiGraphicsOutputProtocolPixelInfo,
    pub size_of_info: u64,
    pub frame_buffer_base: usize,
    pub frame_buffer_size: usize,
    _pinned: PhantomPinned,
}

#[repr(C)]
#[derive(Debug)]
pub struct EfiGraphicsOutputProtocol<'a> {
    reserved: [u64; 3],
    pub mode: &'a EfiGraphicsOutputProtocolMode<'a>,
    _pinned: PhantomPinned,
}

#[repr(C)]
#[derive(Debug)]
pub struct EfiProcessorInformation {
    pub id: u64,
    pub status: u32,
    pub package: u32,
    pub core: u32,
    pub thread: u32,
}

#[repr(C)]
pub struct EfiMPServicesProtocol {
    pub get_number_of_processors: extern "win64" fn(
        this: *const EfiMPServicesProtocol,
        num_of_proc: &mut usize,
        num_of_proc_enabled: &mut usize,
    ) -> EfiStatus,
    pub get_processor_info: extern "win64" fn(
        this: *const EfiMPServicesProtocol,
        proc_num: usize,
        info: &mut EfiProcessorInformation,
    ) -> EfiStatus,
    //
    _pinned: PhantomPinned,
}

pub union EfiBootServicesTableHandleProtocolVariants {
    pub handle_loaded_image_protocol: extern "win64" fn(
        handle: EfiHandle,
        protocol: &EfiGuid,
        interface: *mut Option<Pin<&EfiLoadedImageProtocol>>,
    ) -> EfiStatus,
    pub handle_simple_file_system_protocol: extern "win64" fn(
        handle: EfiHandle,
        protocol: *const EfiGuid,
        interface: &mut Option<Pin<&EfiSimpleFileSystemProtocol>>,
    ) -> EfiStatus,
}

#[allow(dead_code)]
#[repr(usize)]
pub enum AllocType {
    AnyPages = 0,
    MaxAddress,
    Address,
}

#[allow(dead_code)]
#[repr(u32)]
pub enum MemoryType {
    Reserved = 0,
    LoaderCode,
    LoaderData,
    BootServicesCode,
    BootServicesData,
    RuntimeServicesCode,
    RuntimeServicesData,
    ConventionalMemory,
    UnusableMemory,
    ACPIReclaimMemory,
    ACPIMemoryNVS,
    MemoryMappedIO,
    MemoryMappedIOPortSpace,
    PalCode,
    PersistentMemory,
}

#[repr(C)]
pub struct EfiBootServicesTable {
    _header: EfiTableHeader,

    _reserved0: [u64; 2],
    allocate_pages: extern "win64" fn(
        allocate_type: AllocType,
        memory_type: MemoryType,
        pages: usize,
        mem: &mut *mut u8,
    ) -> EfiStatus,
    _reserved1: [u64; 1],
    get_memory_map: extern "win64" fn(
        memory_map_size: *mut usize,
        memory_map: *mut u8,
        map_key: *mut usize,
        descriptor_size: *mut usize,
        descriptor_version: *mut u32,
    ) -> EfiStatus,
    _reserved2: [u64; 11],
    handle_protocol: EfiBootServicesTableHandleProtocolVariants,
    _reserved3: [u64; 9],
    exit_boot_services: extern "win64" fn(image_handle: EfiHandle, map_key: usize) -> EfiStatus,

    _reserved4: [u64; 10],
    locate_protocol: extern "win64" fn(
        protocol: *const EfiGuid,
        registration: *const EfiVoid,
        interface: *mut *mut EfiVoid,
    ) -> EfiStatus,
    //
    _pinned: PhantomPinned,
}

impl EfiBootServicesTable {
    pub fn get_memory_map(&self, map: &mut MemoryMapHolder) -> EfiStatus {
        (self.get_memory_map)(
            &mut map.memory_map_size,
            map.memory_map_buffer.as_mut_ptr(),
            &mut map.map_key,
            &mut map.descriptor_size,
            &mut map.descriptor_version,
        )
    }
    pub fn handle_simple_file_system_protocol(
        &self,
        device_handle: EfiHandle,
    ) -> Result<Pin<&EfiSimpleFileSystemProtocol>> {
        let mut simple_fs_protocol: Option<Pin<&EfiSimpleFileSystemProtocol>> = None;
        unsafe {
            let status = (self.handle_protocol.handle_simple_file_system_protocol)(
                device_handle,
                &constants::EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
                &mut simple_fs_protocol,
            );
            if status != EfiStatus::SUCCESS {
                return Err(status.into());
            }
        }
        simple_fs_protocol.ok_or(Error::Failed("Returned pointer was null"))
    }
    pub fn handle_loaded_image_protocol(
        &self,
        image_handle: EfiHandle,
    ) -> Result<Pin<&'static EfiLoadedImageProtocol>> {
        let mut loaded_image_protocol = None;
        let handle_protocol = &self.handle_protocol;
        let status = unsafe {
            (handle_protocol.handle_loaded_image_protocol)(
                image_handle,
                &constants::EFI_LOADED_IMAGE_PROTOCOL_GUID,
                &mut loaded_image_protocol,
            )
        };
        status.into_result()?;
        loaded_image_protocol.ok_or(Error::Failed("Returned pointer was null"))
    }
}

#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum EfiMemoryType {
    RESERVED = 0,
    LOADER_CODE,
    LOADER_DATA,
    BOOT_SERVICES_CODE,
    BOOT_SERVICES_DATA,
    RUNTIME_SERVICES_CODE,
    RUNTIME_SERVICES_DATA,
    CONVENTIONAL_MEMORY,
    UNUSABLE_MEMORY,
    ACPI_RECLAIM_MEMORY,
    ACPI_MEMORY_NVS,
    MEMORY_MAPPED_IO,
    MEMORY_MAPPED_IO_PORT_SPACE,
    PAL_CODE,
    PERSISTENT_MEMORY,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EfiMemoryDescriptor {
    pub memory_type: EfiMemoryType,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub number_of_pages: u64,
    pub attribute: u64,
}

impl fmt::Debug for EfiMemoryDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EfiMemoryDescriptor")
            .field(
                "phys",
                &format_args!(
                    "[{:#014X}-{:#014X})",
                    self.physical_start,
                    self.physical_start + self.number_of_pages * 4096,
                ),
            )
            .field("size", &format_args!("({:8} pages)", self.number_of_pages))
            .field("attr", &format_args!("{:#X}", self.attribute))
            .field("type", &format_args!("{:?}", self.memory_type))
            .finish()
    }
}

#[repr(C)]
struct EfiConfigurationTable {
    vendor_guid: EfiGuid,
    vendor_table: *const u8,
}

#[repr(C)]
pub struct EfiSystemTable {
    header: EfiTableHeader,
    firmware_vendor: EfiHandle,
    firmware_revision: u32,
    console_in_handle: EfiHandle,
    con_in: EfiHandle,
    console_out_handle: EfiHandle,
    con_out: Pin<&'static EfiSimpleTextOutputProtocol>,
    standard_error_handle: EfiHandle,
    std_err: EfiHandle,
    runtime_services: EfiHandle,
    boot_services: Pin<&'static EfiBootServicesTable>,
    number_of_table_entries: usize,
    configuration_table: *const EfiConfigurationTable,
    //
    _pinned: PhantomPinned,
}
impl EfiSystemTable {
    pub fn con_out(&self) -> Pin<&EfiSimpleTextOutputProtocol> {
        self.con_out
    }
    pub fn boot_services(&self) -> Pin<&EfiBootServicesTable> {
        self.boot_services
    }
    pub fn get_table_with_guid<T>(&self, guid: &EfiGuid) -> Option<&T> {
        for i in 0..self.number_of_table_entries {
            let ct = unsafe { &*self.configuration_table.add(i) };
            if ct.vendor_guid == *guid {
                return Some(unsafe { &*(ct.vendor_table as *const T) });
            }
        }
        None
    }
}
impl fmt::Debug for EfiSystemTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EfiSystemTable")
            .field("header", &format_args!("{:?}", self.header))
            .finish()
    }
}

pub struct EfiSimpleTextOutputProtocolWriter<'a> {
    pub protocol: &'a EfiSimpleTextOutputProtocol,
    //
    _pinned: PhantomPinned,
}

impl EfiSimpleTextOutputProtocolWriter<'_> {
    pub fn write_char(&mut self, c: u8) {
        let cbuf: [u16; 2] = [c.into(), 0];
        (self.protocol.output_string)(self.protocol, cbuf.as_ptr())
            .into_result()
            .unwrap();
    }
    pub fn write_str(&mut self, s: &str) {
        for c in s.bytes() {
            if c == b'\n' {
                self.write_char(b'\r');
            }
            self.write_char(c);
        }
    }
}

impl fmt::Write for EfiSimpleTextOutputProtocolWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_str(s);
        Ok(())
    }
}

pub fn locate_mp_services_protocol<'a>(
    efi_system_table: Pin<&EfiSystemTable>,
) -> Result<&'a EfiMPServicesProtocol> {
    let mut protocol: *mut EfiMPServicesProtocol = null_mut::<EfiMPServicesProtocol>();
    let status = (efi_system_table.boot_services.locate_protocol)(
        &constants::EFI_MP_SERVICES_PROTOCOL_GUID,
        null_mut::<EfiVoid>(),
        &mut protocol as *mut *mut EfiMPServicesProtocol as *mut *mut EfiVoid,
    );
    if status != EfiStatus::SUCCESS {
        return Err("MP services not found".into());
    }
    Ok(unsafe { &*protocol })
}

/// alloc_pages allocates a memory region
/// with EFI_LOADER_DATA type so that the // region stays even after exiting boot services.
pub fn alloc_pages(
    efi_system_table: Pin<&EfiSystemTable>,
    number_of_pages: usize,
) -> Result<*mut u8> {
    let mut mem = null_mut();
    let status = (efi_system_table.as_ref().boot_services.allocate_pages)(
        AllocType::AnyPages,
        MemoryType::LoaderData,
        number_of_pages,
        &mut mem,
    );
    status.into_result()?;
    if mem.is_null() {
        Err(Error::Failed(
            "boot_services.alloc_pages returned null pointer",
        ))
    } else {
        Ok(mem)
    }
}

pub fn alloc_byte_slice<'a>(
    efi_system_table: Pin<&EfiSystemTable>,
    size: usize,
) -> Result<&'a mut [u8]> {
    let pages = alloc_pages(efi_system_table, size_in_pages_from_bytes(size))?;
    Ok(unsafe { core::slice::from_raw_parts_mut(pages, size) })
}

pub fn locate_graphic_protocol<'a>(
    efi_system_table: Pin<&EfiSystemTable>,
) -> Result<&'a EfiGraphicsOutputProtocol<'a>> {
    let mut graphic_output_protocol: *mut EfiGraphicsOutputProtocol =
        null_mut::<EfiGraphicsOutputProtocol>();
    let status = (efi_system_table.as_ref().boot_services.locate_protocol)(
        &constants::EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
        null_mut::<EfiVoid>(),
        &mut graphic_output_protocol as *mut *mut EfiGraphicsOutputProtocol as *mut *mut EfiVoid,
    );
    if status != EfiStatus::SUCCESS {
        return Err("Failed to locate graphics output protocol".into());
    }
    Ok(unsafe { &*graphic_output_protocol })
}

pub fn exit_from_efi_boot_services(
    image_handle: EfiHandle,
    efi_system_table: Pin<&EfiSystemTable>,
    memory_map: &mut memory_map_holder::MemoryMapHolder,
) {
    // Get a memory map and exit boot services
    loop {
        // exit_boot_services can fail if the memory map is updated in the logic so keep retrying in the
        // loop.
        let status = efi_system_table
            .as_ref()
            .boot_services
            .get_memory_map(memory_map);
        assert_eq!(status, EfiStatus::SUCCESS);
        let status = (efi_system_table.as_ref().boot_services.exit_boot_services)(
            image_handle,
            memory_map.map_key,
        );
        if status == EfiStatus::SUCCESS {
            break;
        }
    }
}
