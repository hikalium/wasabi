extern crate alloc;

use crate::efi::types::EfiStatus;
use alloc::string::String;
use core::num::TryFromIntError;
use noli::error::Error as NoliError;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Error {
    EfiError(EfiStatus),
    Failed(&'static str),
    FailedString(String),
    FileNameTooLong,
    GraphicsError,
    PciBusDeviceFunctionOutOfRange,
    ReadFileSizeMismatch { expected: usize, actual: usize },
    ApicRegIndexOutOfRange,
    CalcOutOfRange,
    PageNotFound,
    PciBarInvalid,
    PciEcmOutOfRange,
    TryFromIntError,
    LockFailed,
    NoliError(NoliError),
}
impl From<EfiStatus> for Error {
    fn from(e: EfiStatus) -> Self {
        Error::EfiError(e)
    }
}
impl From<&'static str> for Error {
    fn from(s: &'static str) -> Self {
        Error::Failed(s)
    }
}
impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::FailedString(s)
    }
}
impl From<TryFromIntError> for Error {
    fn from(_: TryFromIntError) -> Self {
        Error::TryFromIntError
    }
}
impl From<NoliError> for Error {
    fn from(e: NoliError) -> Self {
        if e == NoliError::GraphicsOutOfRange {
            return Error::GraphicsError;
        }
        Error::NoliError(e)
    }
}
pub type Result<T> = core::result::Result<T, Error>;
