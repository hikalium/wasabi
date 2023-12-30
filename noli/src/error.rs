extern crate alloc;
use alloc::string::String;
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Error {
    Failed(&'static str),
    FailedString(String),
}
pub type Result<T> = core::result::Result<T, Error>;
