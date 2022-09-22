extern crate alloc;

use crate::arch::x86_64::busy_loop_hint;
use crate::error::Result;
use crate::println;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use core::cell::SyncUnsafeCell;
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

fn dummy_waker() -> Waker {
    unsafe { Waker::from_raw(dummy_raw_waker()) }
}

#[derive(Default)]
pub struct Executor {
    task_queue: VecDeque<Task>,
}
impl Executor {
    pub fn spawn(&mut self, task: Task) {
        self.task_queue.push_back(task)
    }
    pub fn run(executor: Rc<SyncUnsafeCell<Self>>) {
        loop {
            let executor_locked = unsafe { &mut *executor.get() };
            if let Some(mut task) = executor_locked.task_queue.pop_front() {
                let waker = dummy_waker();
                let mut context = Context::from_waker(&waker);
                match task.poll(&mut context) {
                    Poll::Ready(result) => {
                        println!("Task done! {:?}", result);
                    }
                    Poll::Pending => {
                        executor_locked.task_queue.push_back(task);
                    }
                }
            }
        }
    }
}

pub async fn delay() {
    for _ in 0..10000 {
        busy_loop_hint();
    }
}
