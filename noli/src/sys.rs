#[macro_use]
#[cfg(target_os = "none")]
pub mod wasabi;
#[cfg(target_os = "none")]
pub use wasabi as os;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux as os;
#[cfg(target_os = "uefi")]
pub mod uefi;
#[cfg(target_os = "uefi")]
pub use uefi as os;

pub mod api;
