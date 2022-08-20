use crate::efi::*;
use crate::graphics::*;
use core::num::TryFromIntError;

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum WasabiError {
    EfiError(EfiStatus),
    Failed(&'static str),
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
impl From<TryFromIntError> for WasabiError {
    fn from(_: TryFromIntError) -> Self {
        WasabiError::TryFromIntError
    }
}
pub type Result<T> = core::result::Result<T, WasabiError>;
