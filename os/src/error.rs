use crate::efi::EFIStatus;
use graphics::GraphicsError;

#[derive(Debug)]
pub enum WasabiError {
    Failed(),
    GraphicsError(GraphicsError),
    EFIError(EFIStatus),
}

impl From<GraphicsError> for WasabiError {
    fn from(e: GraphicsError) -> Self {
        WasabiError::GraphicsError(e)
    }
}

impl From<EFIStatus> for WasabiError {
    fn from(e: EFIStatus) -> Self {
        WasabiError::EFIError(e)
    }
}
