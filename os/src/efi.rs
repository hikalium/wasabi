use crate::error::*;
use crate::memory_map_holder;
use crate::util::*;
use core::fmt;
use core::ptr::null_mut;

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
                write!(f, "{}", bytes[1] as char)?;
                index += 1;
            }
        }
        Result::Ok(())
    }
}

#[repr(C)]
pub struct EfiGuid {
    data0: u32,
    data1: u16,
    data2: u16,
    data3: [u8; 8],
}

pub const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data0: 0x9042a9de,
    data1: 0x23dc,
    data2: 0x4a38,
    data3: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};

pub const EFI_MP_SERVICES_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data0: 0x3fdda605,
    data1: 0xa76e,
    data2: 0x4f46,
    data3: [0xad, 0x29, 0x12, 0xf4, 0x53, 0x1b, 0x3d, 0x08],
};
pub const EFI_LOADED_IMAGE_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data0: 0x5B1B31A1,
    data1: 0x9562,
    data2: 0x11d2,
    data3: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};
pub const EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data0: 0x0964e5b22,
    data1: 0x6459,
    data2: 0x11d2,
    data3: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};
pub const EFI_FILE_SYSTEM_INFO_GUID: EfiGuid = EfiGuid {
    data0: 0x09576e93,
    data1: 0x6d3f,
    data2: 0x11d2,
    data3: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};

pub type EfiVoid = u8;

#[repr(C)]
pub struct EfiTableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    reserved: u32,
}

pub type EfiHandle = u64;

#[derive(Debug, PartialEq, Copy, Clone)]
#[must_use]
pub enum EfiStatus {
    SUCCESS = 0,
}

type EfiResult = Result<(), EfiStatus>;

impl From<EfiStatus> for EfiResult {
    fn from(e: EfiStatus) -> Self {
        use EfiStatus::*;
        if e == SUCCESS {
            Ok(())
        } else {
            Err(e)
        }
    }
}

impl EfiStatus {
    pub fn into_result(&self) -> EfiResult {
        (*self).into()
    }
}

#[repr(C)]
pub struct EfiSimpleTextOutputProtocol {
    pub reset: EfiHandle,
    pub output_string:
        extern "win64" fn(this: *const EfiSimpleTextOutputProtocol, str: *const u16) -> EfiStatus,
    pub test_string: EfiHandle,
    pub query_mode: EfiHandle,
    pub set_mode: EfiHandle,
    pub set_attribute: EfiHandle,
    pub clear_screen: extern "win64" fn(this: *const EfiSimpleTextOutputProtocol) -> EfiStatus,
}

#[repr(C)]
pub struct EfiLoadedImageProtocol<'a> {
    pub revision: u32,
    pub parent_handle: EfiHandle,
    pub system_table: &'a EfiSystemTable<'a>,
    pub device_handle: EfiHandle,
}

#[repr(C)]
#[derive(Debug)]
pub struct EfiGraphicsOutputProtocolPixelInfo {
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
pub struct EfiGraphicsOutputProtocolMode<'a> {
    pub max_mode: u32,
    pub mode: u32,
    pub info: &'a EfiGraphicsOutputProtocolPixelInfo,
    pub size_of_info: u64,
    pub frame_buffer_base: usize,
    pub frame_buffer_size: usize,
}

#[repr(C)]
#[derive(Debug)]
pub struct EfiGraphicsOutputProtocol<'a> {
    reserved: [u64; 3],
    pub mode: &'a EfiGraphicsOutputProtocolMode<'a>,
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
#[allow(dead_code)]
#[derive(Default, Debug)]
pub struct EfiTime {
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
impl fmt::Display for EfiTime {
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
pub struct EfiFileName {
    pub file_name: [u16; 32],
}
impl fmt::Display for EfiFileName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", CStrPtr16::from_ptr(self.file_name.as_ptr()),)
    }
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct EfiFileInfo {
    pub size: u64,
    pub file_size: u64,
    pub physical_size: u64,
    pub create_time: EfiTime,
    pub last_access_time: EfiTime,
    pub modification_time: EfiTime,
    pub attr: u64,
    pub file_name: EfiFileName,
}
impl EfiFileInfo {
    pub fn is_dir(&self) -> bool {
        self.attr & 0x10 != 0
    }
}
impl fmt::Display for EfiFileInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "EfiFileInfo {{ create_time: {}, attr: {:#X}, file_name: {}, file_size: {}}}",
            self.create_time, self.attr, self.file_name, self.file_size,
        )
    }
}

#[repr(C)]
#[derive(Default)]
pub struct EfiFileSystemInfo {
    pub size: u64,
    pub readonly: u8,
    pub volume_size: u64,
    pub free_space: u64,
    pub block_size: u32,
    pub volume_label: [u16; 32],
}

// [uefi_2_9]:588
#[repr(C)]
pub struct EfiFileProtocol {
    revision: u64,
    open: extern "win64" fn(
        this: *const Self,
        new_handle: &mut *const Self,
        file_name: &EfiFileName,
        open_mode: u64,
        attributes: u64,
    ) -> EfiStatus,
    reserved0: [u64; 2],
    read:
        extern "win64" fn(this: *const Self, buffer_size: &mut usize, buffer: *mut u8) -> EfiStatus,
    reserved1: [u64; 3],
    get_info: extern "win64" fn(
        this: *const Self,
        information_type: *const EfiGuid,
        buffer_size: &mut usize,
        buffer: *mut u8,
    ) -> EfiStatus,
}

impl EfiFileProtocol {
    pub fn open(&self, name: &EfiFileName) -> &EfiFileProtocol {
        let mut new_file_protocol = core::ptr::null::<EfiFileProtocol>();
        let status = (self.open)(
            self as *const EfiFileProtocol,
            &mut new_file_protocol,
            name,
            1, /* Read */
            0,
        );
        assert_eq!(status, EfiStatus::SUCCESS);
        // Safety: this is safe since the object pointed by the pointer is allocated by UEFI and it
        // will be valid upon success
        unsafe { &*new_file_protocol }
    }
    fn read_into_type<T>(&self) -> Option<T> {
        // Safety: data will be initialized in this function and it will be returned only if the
        // UEFI protocol succeeds.
        let mut data = unsafe { core::mem::zeroed::<T>() };
        let buf_size = core::mem::size_of::<T>();
        let mut size_read = buf_size;
        let status = (self.read)(
            self as *const EfiFileProtocol,
            &mut size_read,
            &mut data as *mut T as *mut u8,
        );
        assert_eq!(status, EfiStatus::SUCCESS);
        if size_read > 0 {
            crate::println!("Read {size_read} bytes");
            crate::print::hexdump_struct(&data);
            Some(data)
        } else {
            None
        }
    }
    pub fn read_file_info(&self) -> Option<EfiFileInfo> {
        self.read_into_type::<EfiFileInfo>()
    }
    pub fn read_into_slice<T>(&self, buf: &mut [T]) -> Result<(), WasabiError> {
        let size_expected = buf.len();
        let mut size_read = size_expected;
        let status = (self.read)(
            self as *const EfiFileProtocol,
            &mut size_read,
            buf.as_mut_ptr() as *mut u8,
        );
        assert_eq!(status, EfiStatus::SUCCESS);
        if size_read != size_expected {
            Err(WasabiError::FileRead)
        } else {
            Ok(())
        }
    }
    unsafe fn get_info<T>(&self, information_type: &EfiGuid) -> T {
        let mut data = core::mem::zeroed::<T>();
        let status = (self.get_info)(
            self as *const EfiFileProtocol,
            information_type,
            &mut core::mem::size_of::<T>(),
            &mut data as *mut T as *mut u8,
        );
        assert_eq!(status, EfiStatus::SUCCESS);
        data
    }
    pub fn get_fs_info(&self) -> EfiFileSystemInfo {
        unsafe { self.get_info(&EFI_FILE_SYSTEM_INFO_GUID) }
    }
}

#[repr(C)]
pub struct EfiSimpleFileSystemProtocol {
    revision: u64,
    open_volume:
        extern "win64" fn(this: *const Self, root: *mut *const EfiFileProtocol) -> EfiStatus,
}
impl EfiSimpleFileSystemProtocol {
    pub fn open_volume(&self) -> &EfiFileProtocol {
        let mut new_file_protocol = core::ptr::null::<EfiFileProtocol>();
        let status = (self.open_volume)(self as *const Self, &mut new_file_protocol);
        assert_eq!(status, EfiStatus::SUCCESS);
        unsafe { &*new_file_protocol }
    }
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
}

pub union EfiBootServicesTableHandleProtocolVariants {
    pub handle_loaded_image_protocol: extern "win64" fn(
        handle: EfiHandle,
        protocol: *const EfiGuid,
        interface: *mut *mut EfiLoadedImageProtocol,
    ) -> EfiStatus,
    pub handle_simple_file_system_protocol: extern "win64" fn(
        handle: EfiHandle,
        protocol: *const EfiGuid,
        interface: *mut *mut EfiSimpleFileSystemProtocol,
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

pub struct EfiBootServicesTable {
    pub header: EfiTableHeader,

    _reserved0: [u64; 2],
    pub allocate_pages: extern "win64" fn(
        allocate_type: AllocType,
        memory_type: MemoryType,
        pages: usize,
        mem: &*mut u8,
    ) -> EfiStatus,
    _reserved1: [u64; 1],
    pub get_memory_map: extern "win64" fn(
        memory_map_size: *mut usize,
        memory_map: *mut u8,
        map_key: *mut usize,
        descriptor_size: *mut usize,
        descriptor_version: *mut u32,
    ) -> EfiStatus,
    _reserved2: [u64; 11],
    pub handle_protocol: EfiBootServicesTableHandleProtocolVariants,
    _reserved3: [u64; 9],
    pub exit_boot_services: extern "win64" fn(image_handle: EfiHandle, map_key: usize) -> EfiStatus,

    _reserved4: [u64; 10],
    pub locate_protocol: extern "win64" fn(
        protocol: *const EfiGuid,
        registration: *const EfiVoid,
        interface: *mut *mut EfiVoid,
    ) -> EfiStatus,
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
pub struct EfiSystemTable<'a> {
    pub header: EfiTableHeader,
    pub firmware_vendor: EfiHandle,
    pub firmware_revision: u32,
    pub console_in_handle: EfiHandle,
    pub con_in: EfiHandle,
    pub console_out_handle: EfiHandle,
    pub con_out: &'a EfiSimpleTextOutputProtocol,
    pub standard_error_handle: EfiHandle,
    pub std_err: EfiHandle,
    pub runtime_services: EfiHandle,
    pub boot_services: &'a EfiBootServicesTable,
}

pub struct EfiSimpleTextOutputProtocolWriter<'a> {
    pub protocol: &'a EfiSimpleTextOutputProtocol,
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
    efi_system_table: &'a EfiSystemTable,
) -> Result<&'a EfiMPServicesProtocol, WasabiError> {
    let mut protocol: *mut EfiMPServicesProtocol = null_mut::<EfiMPServicesProtocol>();
    let status = (efi_system_table.boot_services.locate_protocol)(
        &EFI_MP_SERVICES_PROTOCOL_GUID,
        null_mut::<EfiVoid>(),
        &mut protocol as *mut *mut EfiMPServicesProtocol as *mut *mut EfiVoid,
    );
    if status != EfiStatus::SUCCESS {
        return Err(WasabiError::Failed());
    }
    Ok(unsafe { &*protocol })
}

unsafe fn alloc_pages(
    efi_system_table: &EfiSystemTable,
    number_of_pages: usize,
) -> Result<*mut u8, WasabiError> {
    let mut mem: *mut u8 = null_mut::<u8>();
    let status = (efi_system_table.boot_services.allocate_pages)(
        AllocType::AnyPages,
        MemoryType::LoaderData,
        number_of_pages,
        &mut mem,
    );
    status
        .into_result()
        .and(Ok(mem))
        .map_err(WasabiError::EfiError)
}

pub fn alloc_slice<'a>(
    efi_system_table: &'a EfiSystemTable,
    size: usize,
) -> Result<&'a mut [u8], WasabiError> {
    Ok(unsafe {
        core::slice::from_raw_parts_mut(
            alloc_pages(efi_system_table, size_in_pages_from_bytes(size))?,
            size,
        )
    })
}

pub fn locate_graphic_protocol<'a>(
    efi_system_table: &'a EfiSystemTable,
) -> Result<&'a EfiGraphicsOutputProtocol<'a>, WasabiError> {
    let mut graphic_output_protocol: *mut EfiGraphicsOutputProtocol =
        null_mut::<EfiGraphicsOutputProtocol>();
    let status = (efi_system_table.boot_services.locate_protocol)(
        &EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
        null_mut::<EfiVoid>(),
        &mut graphic_output_protocol as *mut *mut EfiGraphicsOutputProtocol as *mut *mut EfiVoid,
    );
    if status != EfiStatus::SUCCESS {
        return Err(WasabiError::Failed());
    }
    Ok(unsafe { &*graphic_output_protocol })
}

pub fn exit_from_efi_boot_services(
    image_handle: EfiHandle,
    efi_system_table: &mut EfiSystemTable,
    memory_map: &mut memory_map_holder::MemoryMapHolder,
) {
    // Get a memory map and exit boot services
    let status = memory_map_holder::get_memory_map(efi_system_table, memory_map);
    assert_eq!(status, EfiStatus::SUCCESS);
    let status =
        (efi_system_table.boot_services.exit_boot_services)(image_handle, memory_map.map_key);
    assert_eq!(status, EfiStatus::SUCCESS);
}
