extern crate alloc;

use crate::error::Result;
use crate::memory::ContiguousPhysicalMemoryPages;
use crate::mutex::Mutex;
use crate::println;
use crate::x86_64::context::ExecutionContext;
use crate::x86_64::paging::PageAttr;
use alloc::collections::VecDeque;
use noli::args::serialize_args;

pub static CURRENT_PROCESS: Mutex<Option<ProcessContext>> = Mutex::new(None, "CURRENT_PROCESS");

pub struct ProcessContext {
    args_region: Option<ContiguousPhysicalMemoryPages>,
    stack_region: ContiguousPhysicalMemoryPages,
    context: ExecutionContext,
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
            context: ExecutionContext::default(),
        })
    }
    pub fn stack(&mut self) -> &mut ContiguousPhysicalMemoryPages {
        &mut self.stack_region
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
    const fn default() -> Self {
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
        let from = queue.pop_back();
        let to = queue
            .get_mut(0)
            .expect("queue should have a process to swith to");
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
        unimplemented!()
        /*
        let mut another_stack = [0u64; 4096];
        let another_func_addr = another_proc_func as *const unsafe extern "sysv64" fn() as u64;
        let rip_to_ret_on_another_stack = another_stack.last_mut().unwrap();
        *rip_to_ret_on_another_stack = another_func_addr;
        CONTEXT_TEST.lock().cpu.rsp = rip_to_ret_on_another_stack as *mut u64 as u64;
        unsafe {
            *ANOTHER_FUNC_COUNT.lock() = 1;
            fork_context(&CONTEXT_MAIN, &CONTEXT_TEST);
            assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 6);
            switch_context(&CONTEXT_MAIN, &CONTEXT_TEST);
            assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 90);
            switch_context(&CONTEXT_MAIN, &CONTEXT_TEST);
            assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 1350);
        }
            */
    }
}
