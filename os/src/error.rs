extern crate alloc;

use crate::efi::EfiStatus;
use crate::graphics::GraphicsError;
use alloc::string::String;
use core::num::TryFromIntError;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WasabiError {
    EfiError(EfiStatus),
    Failed(&'static str),
    FailedString(String),
    FileNameTooLong,
    GraphicsError(GraphicsError),
    PciBusDeviceFunctionOutOfRange,
    ReadFileSizeMismatch { expected: usize, actual: usize },
    ApicRegIndexOutOfRange,
    CalcOutOfRange,
    PageNotFound,
    PciBarInvalid,
    PciEcmOutOfRange,
    TryFromIntError,
    LockFailed,
}
impl From<GraphicsError> for WasabiError {
    fn from(e: GraphicsError) -> Self {
        WasabiError::GraphicsError(e)
    }
}
impl From<EfiStatus> for WasabiError {
    fn from(e: EfiStatus) -> Self {
        WasabiError::EfiError(e)
    }
}
impl From<&'static str> for WasabiError {
    fn from(s: &'static str) -> Self {
        WasabiError::Failed(s)
    }
}
impl From<String> for WasabiError {
    fn from(s: String) -> Self {
        WasabiError::FailedString(s)
    }
}
impl From<TryFromIntError> for WasabiError {
    fn from(_: TryFromIntError) -> Self {
        WasabiError::TryFromIntError
    }
}
pub type Result<T> = core::result::Result<T, WasabiError>;
