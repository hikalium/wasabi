extern crate alloc;

use crate::error::Result;
use crate::memory::ContiguousPhysicalMemoryPages;
use crate::mutex::Mutex;
use crate::println;
use crate::x86_64::context::switch_context;
use crate::x86_64::context::ExecutionContext;
use crate::x86_64::paging::PageAttr;
use alloc::collections::VecDeque;
use noli::args::serialize_args;

pub static CURRENT_PROCESS: Mutex<Option<ProcessContext>> = Mutex::new(None, "CURRENT_PROCESS");

pub struct ProcessContext {
    args_region: Option<ContiguousPhysicalMemoryPages>,
    stack_region: ContiguousPhysicalMemoryPages,
    context: Mutex<ExecutionContext>,
}
impl ProcessContext {
    pub fn new(stack_size: usize, args: Option<&[&str]>) -> Result<Self> {
        let args_region = match args {
            Some(args) => {
                let args = serialize_args(args);
                println!("Serialized args: {args:?}");
                let mut args_region = ContiguousPhysicalMemoryPages::alloc_bytes(args.len())?;
                args_region.fill_with_bytes(0);
                args_region.as_mut_slice()[0..args.len()].copy_from_slice(&args);
                args_region.set_page_attr(PageAttr::ReadWriteUser)?;
                Some(args_region)
            }
            None => None,
        };
        Ok(Self {
            args_region,
            stack_region: ContiguousPhysicalMemoryPages::alloc_bytes(stack_size)?,
            context: Mutex::new(ExecutionContext::default(), "ProcessContext::context"),
        })
    }
    pub fn stack_mut(&mut self) -> &mut ContiguousPhysicalMemoryPages {
        &mut self.stack_region
    }
    pub fn context(&mut self) -> &Mutex<ExecutionContext> {
        &mut self.context
    }
    pub fn args_region_start_addr(&self) -> Option<usize> {
        self.args_region.as_ref().map(|ar| ar.range().start())
    }
}

pub struct Scheduler {
    // The first element is the "current" process
    queue: Mutex<VecDeque<ProcessContext>>,
}
impl Scheduler {
    pub const fn default() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new(), "Scheduler::wait_queue"),
        }
    }
    pub fn fork_process(&mut self) -> ProcessContext {
        unimplemented!()
    }
    pub fn schedule(&mut self, proc: ProcessContext) {
        self.queue.lock().push_back(proc);
    }
    pub fn switch_process(&self) {
        let mut queue = self.queue.lock();
        if queue.len() <= 1 {
            // No process to switch
            return;
        }
        let mut from = queue
            .pop_front()
            .expect("queue should have a process to switch from");
        let to = queue
            .get_mut(0)
            .expect("queue should have a process to swith to");
        unsafe { switch_context(from.context(), to.context()) }
        queue.push_back(from);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    pub static ANOTHER_FUNC_COUNT: Mutex<usize> = Mutex::new(0, "ANOTHER_FUNC_COUNT");
    pub static TEST_SCHEDULER: Scheduler = Scheduler::default();
    fn another_proc_func() {
        *ANOTHER_FUNC_COUNT.lock() *= 2;
        loop {
            *ANOTHER_FUNC_COUNT.lock() *= 3;
            TEST_SCHEDULER.switch_process();
            *ANOTHER_FUNC_COUNT.lock() *= 5;
        }
    }
    #[test_case]
    fn switch_process_works() {
        let mut proc = ProcessContext::new(4096, None).expect("proc creation should succeed");
        let another_func_addr = another_proc_func as *const unsafe extern "sysv64" fn() as u64;
        let stack_slice = proc.stack_mut().as_mut_slice();
        let stack_slice_len = stack_slice.len();
        stack_slice[(stack_slice_len - 8)..].copy_from_slice(&another_func_addr.to_le_bytes());
        let rsp = proc.stack_mut().range().end() - 8;
        proc.context().lock().cpu.rsp = rsp as u64;

        *ANOTHER_FUNC_COUNT.lock() = 1;
        TEST_SCHEDULER.switch_process();
        assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 6);
        TEST_SCHEDULER.switch_process();
        assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 90);
        TEST_SCHEDULER.switch_process();
        assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 1350);
    }
}
