use crate::mutex::Mutex;

pub static CURRENT_PROCESS: Mutex<Option<ProcessContext>> = Mutex::new(None, "CURRENT_PROCESS");

#[repr(C)]
#[derive(Clone, Debug, Default)]
pub struct ProcessContext {}
impl ProcessContext {}
