extern crate alloc;
use crate::error;
use crate::hpet::global_timestamp;
use crate::info;
use crate::mutex::Mutex;
use crate::result::Result;
use crate::x86;
use crate::x86::busy_loop_hint;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::fmt::Debug;
use core::future::Future;
use core::ops::ControlFlow;
use core::panic::Location;
use core::pin::pin;
use core::pin::Pin;
use core::ptr::null;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use core::task::Context;
use core::task::Poll;
use core::task::RawWaker;
use core::task::RawWakerVTable;
use core::task::Waker;
use core::time::Duration;

struct Task<T> {
    future: Pin<Box<dyn Future<Output = Result<T>>>>,
    created_at_file: &'static str,
    created_at_line: u32,
}
impl<T> Task<T> {
    fn new(future: impl Future<Output = Result<T>> + 'static) -> Task<T> {
        Task {
            // Pin the task here to avoid invalidating the self references used
            // in  the future
            future: Box::pin(future),
            created_at_file: Location::caller().file(),
            created_at_line: Location::caller().line(),
        }
    }
    fn poll(&mut self, context: &mut Context) -> Poll<Result<T>> {
        self.future.as_mut().poll(context)
    }
}
impl<T> Debug for Task<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Task({}:{})", self.created_at_file, self.created_at_line)
    }
}

fn no_op_raw_waker() -> RawWaker {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        no_op_raw_waker()
    }
    let vtable = &RawWakerVTable::new(clone, no_op, no_op, no_op);
    RawWaker::new(null::<()>(), vtable)
}
fn no_op_waker() -> Waker {
    unsafe { Waker::from_raw(no_op_raw_waker()) }
}

pub fn block_on<T>(
    future: impl Future<Output = Result<T>> + 'static,
) -> Result<T> {
    let mut task = Task::new(future);
    loop {
        let waker = no_op_waker();
        let mut context = Context::from_waker(&waker);
        match task.poll(&mut context) {
            Poll::Ready(result) => {
                break result;
            }
            Poll::Pending => busy_loop_hint(),
        }
    }
}

pub struct Executor {
    task_queue: Option<VecDeque<Task<()>>>,
}
impl Executor {
    const fn new() -> Self {
        Self { task_queue: None }
    }
    fn task_queue(&mut self) -> &mut VecDeque<Task<()>> {
        if self.task_queue.is_none() {
            self.task_queue = Some(VecDeque::new());
        }
        self.task_queue.as_mut().unwrap()
    }
    fn enqueue(&mut self, task: Task<()>) {
        self.task_queue().push_back(task)
    }
    fn run(executor: &Mutex<Option<Self>>) -> ! {
        loop {
            let task =
                executor.lock().as_mut().map(|e| e.task_queue().pop_front());
            if let Some(Some(mut task)) = task {
                let waker = no_op_waker();
                let mut context = Context::from_waker(&waker);
                match task.poll(&mut context) {
                    Poll::Ready(result) => {
                        info!("Task completed: {:?}: {:?}", task, result);
                    }
                    Poll::Pending => {
                        if let Some(e) = executor.lock().as_mut() {
                            e.task_queue().push_back(task)
                        }
                    }
                }
            }
        }
    }
}
impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct Yield {
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

struct TimeoutFuture {
    time_out: Duration,
}
impl TimeoutFuture {
    fn new(duration: Duration) -> Self {
        Self {
            time_out: global_timestamp() + duration,
        }
    }
}
impl Future for TimeoutFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<()> {
        if self.time_out < global_timestamp() {
            Poll::Ready(())
        } else {
            x86::enable_interrupt();
            x86::hlt();
            x86::disable_interrupt();
            Poll::Pending
        }
    }
}
pub async fn sleep(duration: Duration) {
    TimeoutFuture::new(duration).await
}

pub enum MaybeReadyResult<'a, T, U>
where
    U: Future<Output = T>,
{
    Ready(T),
    NotYet(Pin<&'a mut U>),
}
pub struct MaybeReady<'a, T, U>
where
    U: Future<Output = T>,
{
    wait_on: Option<Pin<&'a mut U>>,
}
impl<'a, T, U> MaybeReady<'a, T, U>
where
    U: Future<Output = T>,
{
    pub fn new(wait_on: Pin<&'a mut U>) -> Self {
        let wait_on = Some(wait_on);
        Self { wait_on }
    }
}
impl<'a, T, U> Future for MaybeReady<'a, T, U>
where
    U: Future<Output = T>,
{
    type Output = MaybeReadyResult<'a, T, U>;
    fn poll(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
    ) -> Poll<MaybeReadyResult<'a, T, U>> {
        if let Some(mut wait_on) = self.wait_on.take() {
            let l = wait_on.as_mut().poll(ctx);
            if let Poll::Ready(l) = l {
                Poll::Ready(MaybeReadyResult::Ready(l))
            } else {
                Poll::Ready(MaybeReadyResult::NotYet(wait_on))
            }
        } else {
            panic!("MaybeReady future polled after taken")
        }
    }
}

pub async fn check_if_ready<T, U>(
    future: Pin<&mut U>,
) -> ControlFlow<Result<T>, Pin<&mut U>>
where
    U: Future<Output = Result<T>>,
{
    match MaybeReady::new(future).await {
        MaybeReadyResult::Ready(l) => ControlFlow::Break(l),
        MaybeReadyResult::NotYet(f) => ControlFlow::Continue(f),
    }
}

pub enum Select2Result<L, R> {
    LeftReady(L),
    RightReady(R),
}

pub struct Select2<'a, L, R> {
    left: Pin<&'a mut dyn Future<Output = L>>,
    right: Pin<&'a mut dyn Future<Output = R>>,
}
impl<'a, L, R> Select2<'a, L, R> {
    pub fn new(
        left: Pin<&'a mut impl Future<Output = L>>,
        right: Pin<&'a mut impl Future<Output = R>>,
    ) -> Self {
        Self { left, right }
    }
}
impl<'a, L, R> Future for Select2<'a, L, R> {
    type Output = Select2Result<L, R>;
    fn poll(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
    ) -> Poll<Select2Result<L, R>> {
        let l = self.left.as_mut().poll(ctx);
        if let Poll::Ready(l) = l {
            return Poll::Ready(Select2Result::LeftReady(l));
        }
        let r = self.right.as_mut().poll(ctx);
        if let Poll::Ready(r) = r {
            return Poll::Ready(Select2Result::RightReady(r));
        }
        Poll::Pending
    }
}

pub async fn with_timeout<T>(
    duration: Duration,
    future: impl Future<Output = Result<T>>,
) -> Result<T> {
    let future = pin!(future);
    let tf = pin!(TimeoutFuture::new(duration));
    match Select2::new(future, tf).await {
        Select2Result::LeftReady(l) => l,
        Select2Result::RightReady(_) => {
            error!(
                "Future at {}:{} is timed out after {:?}",
                Location::caller().file(),
                Location::caller().line(),
                duration
            );
            Err("TimedOut")
        }
    }
}

static GLOBAL_EXECUTOR: Mutex<Option<Executor>> = Mutex::new(None);
#[track_caller]
pub fn spawn_global(future: impl Future<Output = Result<()>> + 'static) {
    let task = Task::new(future);
    GLOBAL_EXECUTOR.lock().get_or_insert_default().enqueue(task);
}
pub fn start_global_executor() -> ! {
    Executor::run(&GLOBAL_EXECUTOR);
}
