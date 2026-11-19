//! A value being built on another thread, so slow set-up (ONNX session, misaki's lexicons)
//! overlaps with other start-up work.

use std::thread::JoinHandle;

pub enum Deferred<T> {
    Pending(Option<JoinHandle<T>>),
    Ready(T),
}

impl<T: Send + 'static> Deferred<T> {
    pub fn spawn(f: impl FnOnce() -> T + Send + 'static) -> Self {
        Self::Pending(Some(std::thread::spawn(f)))
    }

    /// The value, waiting for the thread if needed. A panic on that thread is re-raised here.
    pub fn get(&mut self) -> &mut T {
        if let Self::Pending(handle) = self {
            let value = match handle.take().expect("joined once").join() {
                Ok(v) => v,
                Err(panic) => std::panic::resume_unwind(panic),
            };
            *self = Self::Ready(value);
        }
        match self {
            Self::Ready(v) => v,
            Self::Pending(_) => unreachable!(),
        }
    }
}

impl<T> Drop for Deferred<T> {
    /// Never leave the thread running: exiting while ONNX Runtime is still loading a model on it
    /// crashes in its static destructors.
    fn drop(&mut self) {
        if let Self::Pending(handle) = self
            && let Some(h) = handle.take()
        {
            let _ = h.join();
        }
    }
}
