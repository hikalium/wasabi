extern crate alloc;

use crate::x86::disable_cache;
use alloc::boxed::Box;
use core::marker::PhantomPinned;
use core::mem::ManuallyDrop;
use core::mem::MaybeUninit;
use core::pin::Pin;

pub struct Mmio<T: Sized> {
    inner: ManuallyDrop<Pin<Box<T>>>,
}
impl<T: Sized> Mmio<T> {
    /// # Safety
    /// Caller must ensure:
    /// - *ptr is valid
    /// - CPU Caches for the range pointed by ptr are disabled
    /// - No other party in this program have the ownership of *ptr
    pub unsafe fn from_raw(ptr: *mut T) -> Self {
        Self {
            inner: ManuallyDrop::new(Box::into_pin(Box::from_raw(ptr))),
        }
    }
    /// # Safety
    /// Same rules as Pin::get_unchecked_mut() applies.
    pub unsafe fn get_unchecked_mut(&mut self) -> &mut T {
        self.inner.as_mut().get_unchecked_mut()
    }
}
impl<T> AsRef<T> for Mmio<T> {
    fn as_ref(&self) -> &T {
        self.inner.as_ref().get_ref()
    }
}

#[repr(align(4096))]
pub struct IoBoxInner<T: Sized> {
    data: T,
    _pinned: PhantomPinned,
}
impl<T: Sized> IoBoxInner<T> {
    pub fn new(data: T) -> Self {
        Self {
            data,
            _pinned: PhantomPinned,
        }
    }
}

pub struct IoBox<T: Sized> {
    inner: Pin<Box<IoBoxInner<T>>>,
}
impl<T: Sized> IoBox<T> {
    pub fn new() -> Self {
        let inner = Box::pin(IoBoxInner::new(unsafe {
            MaybeUninit::<T>::zeroed().assume_init()
        }));
        let this = Self { inner };
        disable_cache(&this);
        this
    }
    /// # Safety
    /// Same rules as Pin::get_unchecked_mut() applies.
    pub unsafe fn get_unchecked_mut(&mut self) -> &mut T {
        &mut self.inner.as_mut().get_unchecked_mut().data
    }
}
impl<T> AsRef<T> for IoBox<T> {
    fn as_ref(&self) -> &T {
        &self.inner.as_ref().get_ref().data
    }
}
impl<T: Sized> Default for IoBox<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[test_case]
fn io_box_new() {
    IoBox::<u64>::new();
}
