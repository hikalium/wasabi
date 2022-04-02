use crate::error::*;
use crate::memory_map_holder;
use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct CStrPtr16 {
    ptr: *const u16,
}
impl CStrPtr16 {
    pub fn from_ptr(p: *const u16) -> CStrPtr16 {
        CStrPtr16 { ptr: p }
    }
}

pub fn strlen_char16(strp: CStrPtr16) -> usize {
    let mut len: usize = 0;
    unsafe {
        loop {
            if *strp.ptr.add(len) == 0 {
                break;
            }
            len += 1;
        }
    }
    len
}

impl fmt::Display for CStrPtr16 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        unsafe {
            let mut index = 0;
            loop {
                let c = *self.ptr.offset(index);
                if c == 0 {
                    break;
                }
                let bytes = c.to_be_bytes();
                let result = write!(f, "{}", bytes[1] as char);
                if result.is_err() {
                    return result;
                }
                index += 1;
            }
        }
        Result::Ok(())
    }
}

#[repr(C)]
pub struct EFI_GUID {
    data0: u32,
    data1: u16,
    data2: u16,
    data3: [u8; 8],
}

pub const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: EFI_GUID = EFI_GUID {
    data0: 0x9042a9de,
    data1: 0x23dc,
    data2: 0x4a38,
    data3: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};

pub const EFI_MP_SERVICES_PROTOCOL_GUID: EFI_GUID = EFI_GUID {
    data0: 0x3fdda605,
    data1: 0xa76e,
    data2: 0x4f46,
    data3: [0xad, 0x29, 0x12, 0xf4, 0x53, 0x1b, 0x3d, 0x08],
};
pub const EFI_LOADED_IMAGE_PROTOCOL_GUID: EFI_GUID = EFI_GUID {
    data0: 0x5B1B31A1,
    data1: 0x9562,
    data2: 0x11d2,
    data3: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};
pub const EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID: EFI_GUID = EFI_GUID {
    data0: 0x0964e5b22,
    data1: 0x6459,
    data2: 0x11d2,
    data3: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};
pub const EFI_FILE_SYSTEM_INFO_GUID: EFI_GUID = EFI_GUID {
    data0: 0x09576e93,
    data1: 0x6d3f,
    data2: 0x11d2,
    data3: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};

pub type EFIVoid = u8;
pub type EFINativeUInt = usize;

#[repr(C)]
pub struct EFITableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    reserved: u32,
}

pub type EFIHandle = u64;

#[derive(Debug, PartialEq, Copy, Clone)]
#[must_use]
pub enum EFIStatus {
    SUCCESS = 0,
}

type EFIResult = Result<(), EFIStatus>;

impl From<EFIStatus> for EFIResult {
    fn from(e: EFIStatus) -> Self {
        use EFIStatus::*;
        if e == SUCCESS {
            Ok(())
        } else {
            Err(e)
        }
    }
}

impl EFIStatus {
    pub fn into_result(&self) -> EFIResult {
        (*self).into()
    }
}

#[repr(C)]
pub struct EFISimpleTextOutputProtocol {
    pub reset: EFIHandle,
    pub output_string:
        extern "win64" fn(this: *const EFISimpleTextOutputProtocol, str: *const u16) -> EFIStatus,
    pub test_string: EFIHandle,
    pub query_mode: EFIHandle,
    pub set_mode: EFIHandle,
    pub set_attribute: EFIHandle,
    pub clear_screen: extern "win64" fn(this: *const EFISimpleTextOutputProtocol) -> EFIStatus,
}

#[repr(C)]
pub struct EFILoadedImageProtocol<'a> {
    pub revision: u32,
    pub parent_handle: EFIHandle,
    pub system_table: &'a EFISystemTable<'a>,
    pub device_handle: EFIHandle,
}

#[repr(C)]
#[derive(Debug)]
pub struct EFIGraphicsOutputProtocolPixelInfo {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: u32,
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub reserved_mask: u32,
    pub pixels_per_scan_line: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct EFIGraphicsOutputProtocolMode<'a> {
    pub max_mode: u32,
    pub mode: u32,
    pub info: &'a EFIGraphicsOutputProtocolPixelInfo,
    pub size_of_info: u64,
    pub frame_buffer_base: usize,
    pub frame_buffer_size: usize,
}

#[repr(C)]
#[derive(Debug)]
pub struct EFIGraphicsOutputProtocol<'a> {
    reserved: [u64; 3],
    pub mode: &'a EFIGraphicsOutputProtocolMode<'a>,
}

#[repr(C)]
#[derive(Debug)]
pub struct EFIProcessorInformation {
    pub id: u64,
    pub status: u32,
    pub package: u32,
    pub core: u32,
    pub thread: u32,
}

#[repr(C)]
#[allow(dead_code)]
#[derive(Default, Debug)]
pub struct EFITime {
    year: u16,  // 1900 – 9999
    month: u8,  // 1 – 12
    day: u8,    // 1 – 31
    hour: u8,   // 0 – 23
    minute: u8, // 0 – 59
    second: u8, // 0 – 59
    pad1: u8,
    nanosecond: u32, // 0 – 999,999,999
    time_zone: u16,  // -1440 to 1440 or 2047
    daylight: u8,
    pad2: u8,
}
impl fmt::Display for EFITime {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct EFIFileInfo {
    pub size: u64,
    pub file_size: u64,
    pub physical_size: u64,
    pub create_time: EFITime,
    pub last_access_time: EFITime,
    pub modification_time: EFITime,
    pub attr: u64,
    pub file_name: [u16; 32],
}
impl fmt::Display for EFIFileInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "EFIFileInfo {{ create_time: {}, attr: {:#X}, file_name: {}, file_size: {}}}",
            self.create_time,
            self.attr,
            CStrPtr16::from_ptr(self.file_name.as_ptr()),
            self.file_size,
        )
    }
}

#[repr(C)]
#[derive(Default)]
pub struct EFIFileSystemInfo {
    pub size: u64,
    pub readonly: u8,
    pub volume_size: u64,
    pub free_space: u64,
    pub block_size: u32,
    pub volume_label: [u16; 32],
}

#[repr(C)]
pub struct EFIFileProtocol {
    pub revision: u64,
    reserved0: [u64; 3],
    pub read: extern "win64" fn(
        this: *const EFIFileProtocol,
        buffer_size: &mut EFINativeUInt,
        buffer: &mut EFIFileInfo,
    ) -> EFIStatus,
    reserved1: [u64; 3],
    pub get_info: extern "win64" fn(
        this: *const EFIFileProtocol,
        information_type: *const EFI_GUID,
        buffer_size: &mut EFINativeUInt,
        buffer: &mut EFIFileSystemInfo,
    ) -> EFIStatus,
}

#[repr(C)]
pub struct EFISimpleFileSystemProtocol {
    pub revision: u64,
    pub open_volume: extern "win64" fn(
        this: *const EFISimpleFileSystemProtocol,
        root: *mut *mut EFIFileProtocol,
    ) -> EFIStatus,
}

#[repr(C)]
pub struct EFIMPServicesProtocol {
    pub get_number_of_processors: extern "win64" fn(
        this: *const EFIMPServicesProtocol,
        num_of_proc: &mut usize,
        num_of_proc_enabled: &mut usize,
    ) -> EFIStatus,
    pub get_processor_info: extern "win64" fn(
        this: *const EFIMPServicesProtocol,
        proc_num: usize,
        info: &mut EFIProcessorInformation,
    ) -> EFIStatus,
}

pub union EFIBootServicesTableHandleProtocolVariants {
    pub handle_loaded_image_protocol: extern "win64" fn(
        handle: EFIHandle,
        protocol: *const EFI_GUID,
        interface: *mut *mut EFILoadedImageProtocol,
    ) -> EFIStatus,
    pub handle_simple_file_system_protocol: extern "win64" fn(
        handle: EFIHandle,
        protocol: *const EFI_GUID,
        interface: *mut *mut EFISimpleFileSystemProtocol,
    ) -> EFIStatus,
}

pub struct EFIBootServicesTable {
    pub header: EFITableHeader,

    _reserved0: [u64; 4],
    pub get_memory_map: extern "win64" fn(
        memory_map_size: *mut EFINativeUInt,
        memory_map: *mut u8,
        map_key: *mut EFINativeUInt,
        descriptor_size: *mut EFINativeUInt,
        descriptor_version: *mut u32,
    ) -> EFIStatus,
    _reserved1: [u64; 11],
    pub handle_protocol: EFIBootServicesTableHandleProtocolVariants,
    _reserved2: [u64; 9],
    pub exit_boot_services:
        extern "win64" fn(image_handle: EFIHandle, map_key: EFINativeUInt) -> EFIStatus,

    _reserved3: [u64; 10],
    pub locate_protocol: extern "win64" fn(
        protocol: *const EFI_GUID,
        registration: *const EFIVoid,
        interface: *mut *mut EFIVoid,
    ) -> EFIStatus,
}

#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum EFIMemoryType {
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
pub struct EFIMemoryDescriptor {
    pub memory_type: EFIMemoryType,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub number_of_pages: u64,
    pub attribute: u64,
}

impl fmt::Debug for EFIMemoryDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EFIMemoryDescriptor")
            .field(
                "phys",
                &format_args!(
                    "[{:#018X}-{:#018X})",
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
pub struct EFISystemTable<'a> {
    pub header: EFITableHeader,
    pub firmware_vendor: EFIHandle,
    pub firmware_revision: u32,
    pub console_in_handle: EFIHandle,
    pub con_in: EFIHandle,
    pub console_out_handle: EFIHandle,
    pub con_out: &'a EFISimpleTextOutputProtocol,
    pub standard_error_handle: EFIHandle,
    pub std_err: EFIHandle,
    pub runtime_services: EFIHandle,
    pub boot_services: &'a EFIBootServicesTable,
}

pub struct EFISimpleTextOutputProtocolWriter<'a> {
    pub protocol: &'a EFISimpleTextOutputProtocol,
}

impl EFISimpleTextOutputProtocolWriter<'_> {
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

impl fmt::Write for EFISimpleTextOutputProtocolWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_str(s);
        Ok(())
    }
}

pub fn locate_mp_services_protocol<'a>(
    efi_system_table: &'a EFISystemTable,
) -> Result<&'a EFIMPServicesProtocol, WasabiError> {
    use core::ptr::null_mut;
    let mut protocol: *mut EFIMPServicesProtocol = null_mut::<EFIMPServicesProtocol>();
    let status = (efi_system_table.boot_services.locate_protocol)(
        &EFI_MP_SERVICES_PROTOCOL_GUID,
        null_mut::<EFIVoid>(),
        &mut protocol as *mut *mut EFIMPServicesProtocol as *mut *mut EFIVoid,
    );
    if status != EFIStatus::SUCCESS {
        return Err(WasabiError::Failed());
    }
    Ok(unsafe { &*protocol })
}

pub fn locate_graphic_protocol<'a>(
    efi_system_table: &'a EFISystemTable,
) -> Result<&'a EFIGraphicsOutputProtocol<'a>, WasabiError> {
    use core::ptr::null_mut;
    let mut graphic_output_protocol: *mut EFIGraphicsOutputProtocol =
        null_mut::<EFIGraphicsOutputProtocol>();
    let status = (efi_system_table.boot_services.locate_protocol)(
        &EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
        null_mut::<EFIVoid>(),
        &mut graphic_output_protocol as *mut *mut EFIGraphicsOutputProtocol as *mut *mut EFIVoid,
    );
    if status != EFIStatus::SUCCESS {
        return Err(WasabiError::Failed());
    }
    Ok(unsafe { &*graphic_output_protocol })
}

pub fn exit_from_efi_boot_services(
    image_handle: EFIHandle,
    efi_system_table: &mut EFISystemTable,
    memory_map: &mut memory_map_holder::MemoryMapHolder,
) {
    // Get a memory map and exit boot services
    let status = memory_map_holder::get_memory_map(efi_system_table, memory_map);
    assert_eq!(status, EFIStatus::SUCCESS);
    let status =
        (efi_system_table.boot_services.exit_boot_services)(image_handle, memory_map.map_key);
    assert_eq!(status, EFIStatus::SUCCESS);
}
