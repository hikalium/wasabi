use crate::efi::*;
use crate::graphics::*;

#[derive(Debug)]
pub enum WasabiError {
    Failed(),
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
