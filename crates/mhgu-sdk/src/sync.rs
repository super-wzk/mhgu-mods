//! Serialize state shared by game and render callbacks.
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Shared<T>(AtomicBool, UnsafeCell<T>);
unsafe impl<T: Send> Sync for Shared<T> {}
impl<T> Shared<T> {
    pub const fn new(value: T) -> Self {
        Self(AtomicBool::new(false), UnsafeCell::new(value))
    }
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        while self
            .0
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        let result = f(unsafe { &mut *self.1.get() });
        self.0.store(false, Ordering::Release);
        result
    }
}
