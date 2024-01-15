#![cfg_attr(not(target_os = "linux"), no_std)]
#![feature(alloc_error_handler)]
#[cfg_attr(target_os = "linux", macro_use)]
extern crate std;

pub mod error;
pub mod font;
pub mod graphics;
pub mod net;
pub mod print;
pub mod sys;
pub mod window;
