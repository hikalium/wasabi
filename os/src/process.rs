use crate::error::Result;
use crate::memory::ContiguousPhysicalMemoryPages;
use crate::mutex::Mutex;
use crate::println;
use crate::x86_64::paging::PageAttr;
use noli::args::serialize_args;

pub static CURRENT_PROCESS: Mutex<Option<ProcessContext>> = Mutex::new(None, "CURRENT_PROCESS");

pub struct ProcessContext {
    args_region: ContiguousPhysicalMemoryPages,
}
impl ProcessContext {
    pub fn new(args: &[&str]) -> Result<Self> {
        let args = serialize_args(args);
        println!("Serialized args: {args:?}");
        let mut args_region = ContiguousPhysicalMemoryPages::alloc_bytes(args.len())?;
        args_region.fill_with_bytes(0);
        args_region.as_mut_slice()[0..args.len()].copy_from_slice(&args);
        args_region.set_page_attr(PageAttr::ReadWriteUser)?;
        Ok(ProcessContext { args_region })
    }
    pub fn args_region_start_addr(&self) -> usize {
        self.args_region.range().start()
    }
}
