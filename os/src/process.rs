extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::memory::ContiguousPhysicalMemoryPages;
use crate::mutex::Mutex;
use crate::net::manager::Network;
use crate::net::tcp::TcpSocket;
use crate::x86_64::context::unchecked_load_context;
use crate::x86_64::context::unchecked_switch_context;
use crate::x86_64::context::ExecutionContext;
use crate::x86_64::paging::PageAttr;
use alloc::boxed::Box;
use alloc::collections::btree_map;
use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicI64;
use core::sync::atomic::Ordering;
use core::task::Context;
use core::task::Poll;
use noli::args::serialize_args;
use noli::net::IpV4Addr;

// To take ROOT_SCHEDULER, use Scheduler::root()
static ROOT_SCHEDULER: Scheduler = Scheduler::new();
pub static CURRENT_PROCESS: Mutex<Option<Box<ProcessContext>>> = Mutex::new(None);

pub fn init() {
    ROOT_SCHEDULER.clear_queue();
    ROOT_SCHEDULER.schedule(ProcessContext::default()); // context for current
}

#[derive(Default)]
pub struct ProcessContext {
    args_region: Option<ContiguousPhysicalMemoryPages>,
    stack_region: Option<ContiguousPhysicalMemoryPages>,
    context: Mutex<ExecutionContext>,
    exited: Rc<AtomicBool>,
    exit_code: Rc<AtomicI64>,
    tcp_sockets: BTreeMap<i64, Rc<TcpSocket>>,
    next_tcp_socket_handle: i64,
}
impl ProcessContext {
    pub fn new(
        stack_region: Option<ContiguousPhysicalMemoryPages>,
        args: Option<&[&str]>,
    ) -> Result<Self> {
        let args_region = match args {
            Some(args) => {
                let args = serialize_args(args);
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
            ..Default::default()
        })
    }
    pub fn new_with_fn(f: extern "sysv64" fn(u64), arg1: u64) -> Result<ProcessContext> {
        let mut stack = ContiguousPhysicalMemoryPages::alloc_bytes(1024 * 1024)?;
        let f = f as usize as u64;
        let stack_slice = stack.as_mut_slice();
        let stack_slice_len = stack_slice.len();
        stack_slice[(stack_slice_len - 8)..].copy_from_slice(&f.to_le_bytes());
        let rsp = stack.range().end() - 8;
        let mut proc = ProcessContext::new(Some(stack), None)?;

        proc.context().lock().cpu.rsp = rsp as u64;
        proc.context().lock().cpu.rdi = arg1;
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
    // Create a new tcp socket and issue a handle for it
    pub fn create_tcp_socket(&mut self, ip: IpV4Addr, port: u16) -> Result<i64> {
        let network = Network::take();
        let sock = network.open_tcp_socket(ip, port)?;
        for handle in core::cmp::max(0, self.next_tcp_socket_handle)..=i64::MAX {
            if let btree_map::Entry::Vacant(e) = self.tcp_sockets.entry(handle) {
                e.insert(sock.clone());
                self.next_tcp_socket_handle = self.next_tcp_socket_handle.wrapping_add(1);
                assert!(handle >= 0);
                return Ok(handle);
            }
        }
        Err(Error::Failed("No more tcp_socket handle available"))
    }
    pub fn tcp_socket(&self, handle: i64) -> Option<Rc<TcpSocket>> {
        self.tcp_sockets.get(&handle).cloned()
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
            queue: Mutex::new(VecDeque::new()),
        }
    }
    pub fn schedule(&self, proc: ProcessContext) {
        self.queue.lock().push_back(proc);
    }
    pub fn clear_queue(&self) {
        self.queue.lock().clear();
    }
    pub fn exit_current_process(&self, exit_code: i64) -> ! {
        let to = {
            let mut queue = self.queue.lock();
            if queue.len() <= 1 {
                // No process to switch
                panic!("No more process to schedule!");
            }
            let from = queue
                .pop_front()
                .expect("queue should have a process to exit");
            from.exit_code.store(exit_code, Ordering::SeqCst);
            from.exited.store(true, Ordering::SeqCst);
            let to = unsafe {
                queue
                    .front_mut()
                    .expect("queue should have a process to swith to")
                    .context()
                    .lock()
                    .as_mut_ptr()
            };
            to
        };
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
                    .expect("queue should have a process to swith to");
                if to.exited.load(Ordering::SeqCst) {
                    panic!("trying to switch to exited process...!!!")
                }
                let to = to.context().lock().as_mut_ptr();
                let from = queue
                    .back_mut()
                    .expect("queue should have a process to swith to")
                    .context()
                    .lock()
                    .as_mut_ptr();
                (from, to)
            }
        };
        // The lock for `queue` should be dropped at this point
        unsafe { unchecked_switch_context(from, to) }
    }
}

pub struct ProcessCompletionFuture<'a> {
    exited: Rc<AtomicBool>,
    exit_code: Rc<AtomicI64>,
    scheduler: &'a Scheduler,
}
impl<'a> ProcessCompletionFuture<'a> {
    pub fn new(target: &ProcessContext, scheduler: &'a Scheduler) -> Self {
        Self {
            exited: target.exited.clone(),
            exit_code: target.exit_code.clone(),
            scheduler,
        }
    }
}
impl<'a> Future for ProcessCompletionFuture<'a> {
    type Output = Result<i64>;
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        if self.exited.load(Ordering::SeqCst) {
            Poll::Ready(Ok(self.exit_code.load(Ordering::SeqCst)))
        } else {
            self.scheduler.switch_process();
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::executor::block_on;
    pub static ANOTHER_FUNC_COUNT: Mutex<usize> = Mutex::new(0);
    pub static TEST_SCHEDULER: Scheduler = Scheduler::new();
    extern "sysv64" fn another_proc_func(_: u64) {
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
        let proc = ProcessContext::new_with_fn(another_proc_func, 0)
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
    extern "sysv64" fn proc_func_exit_after_two(_: u64) {
        crate::info!("proc_func_exit_after_two entry");
        {
            *ANOTHER_FUNC_COUNT.lock() *= 2;
        }
        for _ in 0..2 {
            {
                *ANOTHER_FUNC_COUNT.lock() *= 3;
            }
            TEST_SCHEDULER.switch_process();
            {
                *ANOTHER_FUNC_COUNT.lock() *= 5;
            }
        }
        crate::info!("proc_func_exit_after_two loop exit");
        TEST_SCHEDULER.exit_current_process(0)
    }
    #[test_case]
    fn await_process_exit() {
        let proc = ProcessContext::new_with_fn(proc_func_exit_after_two, 0)
            .expect("Proc creation should succeed");
        TEST_SCHEDULER.clear_queue();
        TEST_SCHEDULER.schedule(ProcessContext::default()); // context for current
        let wait = ProcessCompletionFuture::new(&proc, &TEST_SCHEDULER);
        TEST_SCHEDULER.schedule(proc);

        *ANOTHER_FUNC_COUNT.lock() = 1;
        crate::info!("block_on start");
        let res = block_on(wait);
        crate::info!("block_on end. res = {res:?}");

        assert_eq!(*ANOTHER_FUNC_COUNT.lock(), 450);
    }
    extern "sysv64" fn proc_func_with_arg(arg1: u64) {
        assert!(arg1 == 42);
        TEST_SCHEDULER.exit_current_process(0)
    }
    #[test_case]
    fn await_process_with_param() {
        let proc = ProcessContext::new_with_fn(proc_func_with_arg, 42)
            .expect("Proc creation should succeed");
        TEST_SCHEDULER.clear_queue();
        TEST_SCHEDULER.schedule(ProcessContext::default()); // context for current
        let wait = ProcessCompletionFuture::new(&proc, &TEST_SCHEDULER);
        TEST_SCHEDULER.schedule(proc);
        assert_eq!(block_on(wait), Ok(0));
    }
}
