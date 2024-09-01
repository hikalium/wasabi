extern crate alloc;
use alloc::string::String;
use core::fmt::Debug;
use core::result::Result as ResultTrait;

pub trait ErrorTrait: Debug {}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Error {
    Failed(&'static str),
    FailedString(String),
    GraphicsOutOfRange,
}

impl ErrorTrait for Error {}
pub type Result<T> = core::result::Result<T, Error>;

pub trait MainReturn {
    fn as_return_code(&self) -> u64;
}
impl MainReturn for () {
    fn as_return_code(&self) -> u64 {
        0
    }
}
impl MainReturn for u64 {
    fn as_return_code(&self) -> u64 {
        *self
    }
}
impl<E: ErrorTrait> MainReturn for ResultTrait<(), E> {
    fn as_return_code(&self) -> u64 {
        if self.is_ok() {
            0
        } else {
            1
        }
    }
}
