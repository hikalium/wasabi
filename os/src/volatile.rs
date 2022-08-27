use core::ptr::read_volatile;
use core::ptr::write_volatile;

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct Volatile<T> {
    _value: T,
}
impl<T> Volatile<T> {
    pub fn read(&self) -> T {
        unsafe { read_volatile(&self._value) }
    }
    pub fn write(&mut self, new_value: T) {
        unsafe { write_volatile(&mut self._value, new_value) }
    }
}
