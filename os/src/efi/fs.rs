use crate::error::Error;
use crate::error::Result;
use core::fmt;
use core::marker::PhantomPinned;
use core::mem::size_of;
use core::mem::zeroed;
use core::pin::Pin;
use core::ptr::null;

use crate::efi::constants;
use crate::efi::types::CStrPtr16;
use crate::efi::types::EfiGuid;
use crate::efi::types::EfiStatus;
use crate::efi::types::EfiTime;

#[repr(C)]
#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
pub struct EfiFileName {
    name: [u16; 32],
}
impl EfiFileName {
    pub fn name(&self) -> &[u16; 32] {
        &self.name
    }
}
impl fmt::Display for EfiFileName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", CStrPtr16::from_ptr(self.name.as_ptr()),)
    }
}
impl core::str::FromStr for EfiFileName {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let src = s.encode_utf16();
        let mut dst = [0u16; 32];
        if src.clone().count() > dst.len() {
            Err(Error::Failed("too long for EfiFileName"))
        } else {
            dst.iter_mut().zip(src).for_each(|(d, s)| *d = s);
            Ok(Self { name: dst })
        }
    }
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct EfiFileInfo {
    size: u64,
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
    pub fn file_size(&self) -> usize {
        self.file_size as usize
    }
    pub fn file_name(&self) -> EfiFileName {
        self.file_name
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
    //
    _pinned: PhantomPinned,
}

impl EfiFileProtocol {
    pub fn open(&self, name: &EfiFileName) -> &EfiFileProtocol {
        let mut new_file_protocol = null::<EfiFileProtocol>();
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
        let mut data = unsafe { zeroed::<T>() };
        let buf_size = size_of::<T>();
        let mut size_read = buf_size;
        let status = (self.read)(
            self as *const EfiFileProtocol,
            &mut size_read,
            &mut data as *mut T as *mut u8,
        );
        assert_eq!(status, EfiStatus::SUCCESS);
        if size_read > 0 {
            Some(data)
        } else {
            None
        }
    }
    pub fn read_file_info(&self) -> Option<EfiFileInfo> {
        self.read_into_type::<EfiFileInfo>()
    }
    pub fn read_into_slice<T>(&self, buf: &mut [T]) -> Result<()> {
        let size_expected = buf.len();
        let mut size_read = size_expected;
        let status = (self.read)(
            self as *const EfiFileProtocol,
            &mut size_read,
            buf.as_mut_ptr() as *mut u8,
        );
        assert_eq!(status, EfiStatus::SUCCESS);
        if size_read != size_expected {
            Err(Error::ReadFileSizeMismatch {
                expected: size_expected,
                actual: size_read,
            })
        } else {
            Ok(())
        }
    }
    unsafe fn get_info<T>(&self, information_type: &EfiGuid) -> T {
        let mut data = zeroed::<T>();
        let status = (self.get_info)(
            self as *const EfiFileProtocol,
            information_type,
            &mut size_of::<T>(),
            &mut data as *mut T as *mut u8,
        );
        assert_eq!(status, EfiStatus::SUCCESS);
        data
    }
    pub fn get_fs_info(&self) -> EfiFileSystemInfo {
        unsafe { self.get_info(&constants::EFI_FILE_SYSTEM_INFO_GUID) }
    }
}

#[repr(C)]
pub struct EfiSimpleFileSystemProtocol {
    revision: u64,
    open_volume:
        extern "win64" fn(this: *const Self, root: &mut Option<Pin<&EfiFileProtocol>>) -> EfiStatus,
    //
    _pinned: PhantomPinned,
}
impl EfiSimpleFileSystemProtocol {
    pub fn open_volume(&self) -> Result<Pin<&EfiFileProtocol>> {
        let mut fp = None;
        let status = (self.open_volume)(self as *const Self, &mut fp);

        status.into_result()?;
        fp.ok_or(Error::Failed("returned pointer was null"))
    }
}
