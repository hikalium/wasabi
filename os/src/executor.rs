extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::hpet::Hpet;
use crate::mutex::Mutex;
use crate::println;
use crate::process::Scheduler;
use crate::x86_64::busy_loop_hint;
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

pub struct Task<T> {
    future: Pin<Box<dyn Future<Output = Result<T>>>>,
}
impl<T> Task<T> {
    pub fn new(future: impl Future<Output = Result<T>> + 'static) -> Task<T> {
        Task {
            // Pin the task here to avoid invalidating the self references used in  the future
            future: Box::pin(future),
        }
    }
    fn poll(&mut self, context: &mut Context) -> Poll<Result<T>> {
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
    let task = Task::new(future);
    ROOT_EXECUTOR.lock().spawn(task);
}

pub fn block_on<T>(future: impl Future<Output = Result<T>> + 'static) -> Result<T> {
    let mut task = Task::new(future);
    loop {
        let waker = dummy_waker();
        let mut context = Context::from_waker(&waker);
        match task.poll(&mut context) {
            Poll::Ready(result) => {
                break result;
            }
            Poll::Pending => busy_loop_hint(),
        }
    }
}

pub fn block_on_and_schedule<T>(future: impl Future<Output = Result<T>> + 'static) -> Result<T> {
    let mut task = Task::new(future);
    loop {
        let waker = dummy_waker();
        let mut context = Context::from_waker(&waker);
        match task.poll(&mut context) {
            Poll::Ready(result) => {
                break result;
            }
            Poll::Pending => Scheduler::root().switch_process(),
        }
    }
}

pub struct Executor {
    task_queue: Option<VecDeque<Task<()>>>,
}
impl Executor {
    const fn default() -> Self {
        Self { task_queue: None }
    }
    fn task_queue(&mut self) -> &mut VecDeque<Task<()>> {
        if self.task_queue.is_none() {
            self.task_queue = Some(VecDeque::new());
        }
        self.task_queue.as_mut().unwrap()
    }
    pub fn spawn(&mut self, task: Task<()>) {
        self.task_queue().push_back(task)
    }
    pub fn poll(executor: &Mutex<Self>) {
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

pub struct SelectFuture<T: Future, U: Future> {
    left: Mutex<Pin<Box<T>>>,
    right: Mutex<Pin<Box<U>>>,
}
impl<T: Future, U: Future> SelectFuture<T, U> {
    pub fn new(left: T, right: U) -> Self {
        let left = Mutex::new(Box::pin(left), "SelectFuture::left");
        let right = Mutex::new(Box::pin(right), "SelectFuture::right");
        Self { left, right }
    }
}
impl<T: Future, U: Future> Future for SelectFuture<T, U> {
    type Output = (Option<T::Output>, Option<U::Output>);
    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        if let Poll::Ready(left) = self.left.lock().as_mut().poll(ctx) {
            Poll::Ready((Some(left), None))
        } else if let Poll::Ready(right) = self.right.lock().as_mut().poll(ctx) {
            Poll::Ready((None, Some(right)))
        } else {
            Poll::Pending
        }
    }
}

pub async fn with_timeout_ms<F: Future>(f: F, timeout: u64) -> Result<F::Output> {
    let t = TimeoutFuture::new_ms(timeout);
    let (_, res) = SelectFuture::new(t, f).await;
    res.ok_or(Error::Failed("Timed out"))
}
