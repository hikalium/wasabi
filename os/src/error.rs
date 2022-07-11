use crate::efi::*;
use crate::graphics::*;

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
pub type Result<T> = core::result::Result<T, WasabiError>;
