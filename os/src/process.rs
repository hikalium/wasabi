extern crate alloc;

use crate::error::Result;
use crate::memory::ContiguousPhysicalMemoryPages;
use crate::mutex::Mutex;
use crate::println;
use crate::x86_64::context::unchecked_load_context;
use crate::x86_64::context::unchecked_switch_context;
use crate::x86_64::context::ExecutionContext;
use crate::x86_64::paging::PageAttr;
use alloc::collections::VecDeque;
use noli::args::serialize_args;

static ROOT_SCHEDULER: Scheduler = Scheduler::new();
pub static CURRENT_PROCESS: Mutex<Option<ProcessContext>> = Mutex::new(None, "CURRENT_PROCESS");

pub fn init() {
    ROOT_SCHEDULER.clear_queue();
    ROOT_SCHEDULER.schedule(ProcessContext::default()); // context for current
}

pub struct ProcessContext {
    args_region: Option<ContiguousPhysicalMemoryPages>,
    stack_region: Option<ContiguousPhysicalMemoryPages>,
    context: Mutex<ExecutionContext>,
}
impl Default for ProcessContext {
    fn default() -> Self {
        Self {
            args_region: None,
            stack_region: None,
            context: Mutex::new(ExecutionContext::default(), "ProcessContext::context"),
        }
    }
}
impl ProcessContext {
    pub fn new(
        stack_region: Option<ContiguousPhysicalMemoryPages>,
        args: Option<&[&str]>,
    ) -> Result<Self> {
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
            stack_region,
            context: Mutex::new(ExecutionContext::default(), "ProcessContext::context"),
        })
    }
    pub fn new_with_fn(f: *const unsafe extern "sysv64" fn()) -> Result<ProcessContext> {
        let mut stack = ContiguousPhysicalMemoryPages::alloc_bytes(4096)?;
        let f = f as u64;
        let stack_slice = stack.as_mut_slice();
        let stack_slice_len = stack_slice.len();
        stack_slice[(stack_slice_len - 8)..].copy_from_slice(&f.to_le_bytes());
        let rsp = stack.range().end() - 8;
        let mut proc = ProcessContext::new(Some(stack), None)?;

        proc.context().lock().cpu.rsp = rsp as u64;
        proc.context().lock().cpu.rflags = 2;
        Ok(proc)
    }
    pub fn stack_mut(&mut self) -> Option<&mut ContiguousPhysicalMemoryPages> {
        self.stack_region.as_mut()
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
    pub fn root() -> &'static Self {
        &ROOT_SCHEDULER
    }
    pub const fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new(), "Scheduler::wait_queue"),
        }
    }
    pub fn schedule(&self, proc: ProcessContext) {
        self.queue.lock().push_back(proc);
    }
    pub fn clear_queue(&self) {
        self.queue.lock().clear();
    }
    pub fn exit_current_process(&self) -> ! {
        let (from, to) = {
            let mut queue = self.queue.lock();
            if queue.len() <= 1 {
                // No process to switch
                panic!("No more process to schedule!");
            }
            let from = queue
                .pop_front()
                .expect("queue should have a process to exit");
            let to = unsafe {
                queue
                    .front_mut()
                    .expect("queue should have a process to swith to")
                    .context()
                    .lock()
                    .as_mut_ptr()
            };
            (from, to)
        };
        #[allow(clippy::drop_non_drop)]
        core::mem::drop(from);
        unsafe { unchecked_load_context(to) };
        unreachable!("Nothing should come back here");
    }
    pub fn switch_process(&self) {
        let (from, to) = {
            // To make sure the lock is unlocked before the
            // context switch, do this in a block.
            let mut queue = self.queue.lock();
            if queue.len() <= 1 {
                // No process to switch
                return;
            }
            queue.rotate_left(1);
            // SAFETY: to and from is valid until the context switch happens. Also, the execution
            // should not be interrupted until the context switch completes.
            unsafe {
                let to = queue
                    .front_mut()
                    .expect("queue should have a process to swith to")
                    .context()
                    .lock()
                    .as_mut_ptr();
                let from = queue
                    .back_mut()
                    .expect("queue should have a process to swith to")
                    .context()
                    .lock()
                    .as_mut_ptr();
                (from, to)
            }
        };
        crate::info!("switch_process!");
        // The lock for `queue` should be dropped at this point
        unsafe { unchecked_switch_context(from, to) }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    pub static ANOTHER_FUNC_COUNT: Mutex<usize> = Mutex::new(0, "ANOTHER_FUNC_COUNT");
    pub static TEST_SCHEDULER: Scheduler = Scheduler::new();
    fn another_proc_func() {
        crate::info!("another_proc_func entry");
        *ANOTHER_FUNC_COUNT.lock() *= 2;
        loop {
            *ANOTHER_FUNC_COUNT.lock() *= 3;
            TEST_SCHEDULER.switch_process();
            *ANOTHER_FUNC_COUNT.lock() *= 5;
        }
    }
    #[test_case]
    fn switch_process_works() {
        let proc =
            ProcessContext::new_with_fn(another_proc_func as *const unsafe extern "sysv64" fn())
                .expect("Proc creation should succeed");
        TEST_SCHEDULER.clear_queue();
        TEST_SCHEDULER.schedule(ProcessContext::default()); // context for current
        TEST_SCHEDULER.schedule(proc);

        *ANOTHER_FUNC_COUNT.lock() = 1;
        TEST_SCHEDULER.switch_process();
        assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 6);
        TEST_SCHEDULER.switch_process();
        assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 90);
        TEST_SCHEDULER.switch_process();
        assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 1350);
    }
    fn proc_func_exit_after_two() {
        crate::info!("proc_func_exit_after_two entry");
        *ANOTHER_FUNC_COUNT.lock() *= 2;
        for _ in 0..2 {
            *ANOTHER_FUNC_COUNT.lock() *= 3;
            TEST_SCHEDULER.switch_process();
            *ANOTHER_FUNC_COUNT.lock() *= 5;
        }
        crate::info!("proc_func_exit_after_two loop exit");
        TEST_SCHEDULER.exit_current_process()
    }
    #[test_case]
    fn await_process_exit() {
        let proc = ProcessContext::new_with_fn(
            proc_func_exit_after_two as *const unsafe extern "sysv64" fn(),
        )
        .expect("Proc creation should succeed");
        TEST_SCHEDULER.clear_queue();
        TEST_SCHEDULER.schedule(ProcessContext::default()); // context for current
        TEST_SCHEDULER.schedule(proc);

        *ANOTHER_FUNC_COUNT.lock() = 1;
        TEST_SCHEDULER.switch_process();
        assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 6);
        TEST_SCHEDULER.switch_process();
        assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 90);
        TEST_SCHEDULER.switch_process();
        assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 450);
    }
}
