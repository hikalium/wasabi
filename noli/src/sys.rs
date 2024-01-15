#[cfg(not(target_os = "linux"))]
#[macro_use]
#[cfg(not(target_os = "linux"))]
pub mod wasabi;
#[cfg(not(target_os = "linux"))]
pub use wasabi::*;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
