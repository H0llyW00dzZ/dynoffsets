#![allow(dead_code)]

#[allow(unused_imports)]
pub use spin::Mutex;

pub struct OnceCell<T> {
    inner: spin::Once<T>,
}

impl<T> OnceCell<T> {
    pub const fn new() -> Self {
        Self {
            inner: spin::Once::new(),
        }
    }

    #[inline]
    pub fn get(&self) -> Option<&T> {
        self.inner.get()
    }

    #[inline]
    pub fn set(&self, value: T) -> Result<(), T> {
        if self.inner.is_completed() {
            return Err(value);
        }
        let mut slot = Some(value);
        self.inner.call_once(|| slot.take().expect("slot"));
        match slot {
            Some(v) => Err(v),
            None => Ok(()),
        }
    }

    #[inline]
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        self.inner.call_once(f)
    }
}

impl<T> Default for OnceCell<T> {
    fn default() -> Self {
        Self::new()
    }
}
