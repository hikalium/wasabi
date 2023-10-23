extern crate alloc;

use crate::error::Result;
use crate::hpet::Hpet;
use crate::mutex::Mutex;
use crate::println;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::future::Future;
use core::pin::Pin;
use core::ptr::null;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use core::task::Context;
use core::task::Poll;
use core::task::RawWaker;
use core::task::RawWakerVTable;
use core::task::Waker;

#[derive(Default)]
pub struct Yield {
    polled: AtomicBool,
}
impl Future for Yield {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<()> {
        if self.polled.fetch_or(true, Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
pub async fn yield_execution() {
    Yield::default().await
}

pub struct Task {
    future: Pin<Box<dyn Future<Output = Result<()>>>>,
}
impl Task {
    pub fn new(future: impl Future<Output = Result<()>> + 'static) -> Task {
        Task {
            // Pin the task here to avoid invalidating the self references used in  the future
            future: Box::pin(future),
        }
    }
    fn poll(&mut self, context: &mut Context) -> Poll<Result<()>> {
        self.future.as_mut().poll(context)
    }
}
// Do nothing, just no_ops.
fn dummy_raw_waker() -> RawWaker {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        dummy_raw_waker()
    }

    let vtable = &RawWakerVTable::new(clone, no_op, no_op, no_op);
    RawWaker::new(null::<()>(), vtable)
}

pub fn dummy_waker() -> Waker {
    unsafe { Waker::from_raw(dummy_raw_waker()) }
}
pub static ROOT_EXECUTOR: Mutex<Executor> = Mutex::new(Executor::default(), "ROOT_EXECUTOR");
pub fn spawn_global(future: impl Future<Output = Result<()>> + 'static) {
    let mut executor = ROOT_EXECUTOR.lock();
    executor.spawn(Task::new(future));
}

pub struct Executor {
    task_queue: Option<VecDeque<Task>>,
}
impl Executor {
    const fn default() -> Self {
        Self { task_queue: None }
    }
    fn task_queue(&mut self) -> &mut VecDeque<Task> {
        if self.task_queue.is_none() {
            self.task_queue = Some(VecDeque::new());
        }
        self.task_queue.as_mut().unwrap()
    }
    pub fn spawn(&mut self, task: Task) {
        self.task_queue().push_back(task)
    }
    pub fn run(executor: &Mutex<Self>) {
        loop {
            let mut executor_locked = executor.lock();
            if let Some(mut task) = executor_locked.task_queue().pop_front() {
                let waker = dummy_waker();
                let mut context = Context::from_waker(&waker);
                match task.poll(&mut context) {
                    Poll::Ready(result) => {
                        println!("Task done! {:?}", result);
                    }
                    Poll::Pending => {
                        executor_locked.task_queue().push_back(task);
                    }
                }
            }
        }
    }
}

pub struct TimeoutFuture {
    time_out: u64,
}
impl TimeoutFuture {
    pub fn new_ms(timeout_ms: u64) -> Self {
        let time_out = Hpet::take().main_counter() + Hpet::take().freq() / 1000 * timeout_ms;
        Self { time_out }
    }
}
impl Future for TimeoutFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<()> {
        let time_out = self.time_out;
        if time_out < Hpet::take().main_counter() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
