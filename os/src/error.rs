extern crate alloc;

use crate::efi::types::EfiStatus;
use crate::graphics::GraphicsError;
use alloc::string::String;
use core::num::TryFromIntError;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Error {
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
impl From<GraphicsError> for Error {
    fn from(e: GraphicsError) -> Self {
        Error::GraphicsError(e)
    }
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
pub type Result<T> = core::result::Result<T, Error>;
