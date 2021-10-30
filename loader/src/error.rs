use graphics::GraphicsError;

#[derive(Debug)]
pub enum WasabiError {
    Failed(),
    GraphicsError(graphics::GraphicsError),
}

impl From<GraphicsError> for WasabiError {
    fn from(error: GraphicsError) -> Self {
        WasabiError::GraphicsError(error)
    }
}
