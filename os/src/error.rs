use crate::efi::*;
use crate::graphics::*;

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum WasabiError {
    Failed(&'static str),
    GraphicsError(GraphicsError),
    EfiError(EfiStatus),
    FileNameTooLong,
    ReadFileSizeMismatch { expected: usize, actual: usize },
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
