//! Simple thread-safe mutex
//!
//! As the doc of SyncUnsafeCell says,
//! `SyncUnsafeCell::get()` can be used to get
//! `*mut T` from `&SyncUnsafeCell<T>` but we must
//! ensure that the access to the object pointed
//! is unique before dereferencing it.
//!
//! This mutex protects the data with AtomicBool
//! to ensure that the access to the contents
//! is unique so taking a mutable reference
//! to it will be safe.

use crate::result::Result;
use core::cell::SyncUnsafeCell;
use core::fmt::Debug;
use core::ops::Deref;
use core::ops::DerefMut;
use core::panic::Location;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
    data: &'a mut T,
    location: Location<'a>,
}
impl<'a, T> MutexGuard<'a, T> {
    #[track_caller]
    unsafe fn new(mutex: &'a Mutex<T>, data: &SyncUnsafeCell<T>) -> Self {
        Self {
            mutex,
            data: &mut *data.get(),
            location: *Location::caller(),
        }
    }
}
unsafe impl<'a, T> Sync for MutexGuard<'a, T> {}
impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.data
    }
}
impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}
impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mutex.is_taken.store(false, Ordering::SeqCst)
    }
}
impl<'a, T> Debug for MutexGuard<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "MutexGuard {{ location: {:?} }}", self.location)
    }
}

pub struct Mutex<T> {
    data: SyncUnsafeCell<T>,
    is_taken: AtomicBool,
    taker_line_num: AtomicU32,
    created_at_file: &'static str,
    created_at_line: u32,
}
impl<T: Sized> Debug for Mutex<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Mutex @ {}:{}",
            self.created_at_file, self.created_at_line
        )
    }
}
impl<T: Sized> Mutex<T> {
    #[track_caller]
    pub const fn new(data: T) -> Self {
        Self {
            data: SyncUnsafeCell::new(data),
            is_taken: AtomicBool::new(false),
            taker_line_num: AtomicU32::new(0),
            created_at_file: Location::caller().file(),
            created_at_line: Location::caller().line(),
        }
    }
    #[track_caller]
    fn try_lock(&self) -> Result<MutexGuard<T>> {
        if self
            .is_taken
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.taker_line_num
                .store(Location::caller().line(), Ordering::SeqCst);
            Ok(unsafe { MutexGuard::new(self, &self.data) })
        } else {
            Err("Lock failed")
        }
    }
    #[track_caller]
    pub fn lock(&self) -> MutexGuard<T> {
        for _ in 0..10000 {
            if let Ok(locked) = self.try_lock() {
                return locked;
            }
        }
        panic!(
            "Failed to lock Mutex at {}:{}, caller: {:?}, taker_line_num: {}",
            self.created_at_file,
            self.created_at_line,
            Location::caller(),
            self.taker_line_num.load(Ordering::SeqCst),
        )
    }
    pub fn under_locked<R: Sized>(&self, f: &dyn Fn(&mut T) -> Result<R>) -> Result<R> {
        let mut locked = self.lock();
        f(&mut *locked)
    }
}
unsafe impl<T> Sync for Mutex<T> {}
impl<T: Default> Default for Mutex<T> {
    #[track_caller]
    fn default() -> Self {
        Self::new(T::default())
    }
}
