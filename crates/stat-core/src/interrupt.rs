//! Stopping a long computation from outside, and reporting its progress.
//!
//! The application runs some analyses on background threads and lets the
//! user cancel them. It installs an [`Interrupt`] for the thread doing the
//! work with [`with`]; routines with long loops (trees of a forest, rounds of
//! boosting, sampler draws, resampling replicates) call [`check`] once per
//! iteration, which returns [`StatError::Cancelled`] once the interrupt is
//! set, and [`Interrupt::step`] to count progress. Without an installed
//! interrupt both cost one thread-local read and never stop anything, so the
//! routines behave exactly as before when called directly.
//!
//! Loops that run on other threads (rayon) read the interrupt with
//! [`current`] before spreading out and check it themselves.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::error::{Result, StatError};

/// A cancel flag and a progress count shared between a computation and the
/// code that started it.
#[derive(Debug, Default)]
pub struct Interrupt {
    cancelled: AtomicBool,
    done: AtomicUsize,
    total: AtomicUsize,
}

impl Interrupt {
    /// A fresh interrupt: not cancelled, no progress.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Ask the computation to stop at its next check.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// `true` once [`Self::cancel`] was called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Start counting a stage of `total` steps (resets the count).
    pub fn begin(&self, total: usize) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }

    /// One more step done.
    pub fn step(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    /// Steps done and expected in the current stage (`total` 0: unknown).
    #[must_use]
    pub fn progress(&self) -> (usize, usize) {
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }

    /// `Err(Cancelled)` once cancelled.
    ///
    /// # Errors
    /// [`StatError::Cancelled`] after [`Self::cancel`].
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() { Err(StatError::Cancelled) } else { Ok(()) }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Interrupt>>> = const { RefCell::new(None) };
}

/// Run `f` with `interrupt` installed for this thread (the previous one,
/// if any, is put back afterwards, also when `f` panics).
pub fn with<R>(interrupt: Arc<Interrupt>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Arc<Interrupt>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = previous);
        }
    }
    let previous = CURRENT.with(|c| c.borrow_mut().replace(interrupt));
    let _restore = Restore(previous);
    f()
}

/// The interrupt installed for this thread, if any.
#[must_use]
pub fn current() -> Option<Arc<Interrupt>> {
    CURRENT.with(|c| c.borrow().clone())
}

/// `Err(Cancelled)` if this thread's computation was cancelled.
///
/// # Errors
/// [`StatError::Cancelled`] once the installed interrupt is cancelled.
pub fn check() -> Result<()> {
    CURRENT.with(|c| c.borrow().as_ref().map_or(Ok(()), |i| i.check()))
}

/// Start counting a stage of `total` steps on this thread's interrupt.
pub fn begin(total: usize) {
    CURRENT.with(|c| {
        if let Some(i) = c.borrow().as_ref() {
            i.begin(total);
        }
    });
}

/// Count one step on this thread's interrupt.
pub fn step() {
    CURRENT.with(|c| {
        if let Some(i) = c.borrow().as_ref() {
            i.step();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_pass_without_an_interrupt_and_stop_once_cancelled() {
        assert!(check().is_ok());
        step();
        let i = Interrupt::new();
        let out = with(Arc::clone(&i), || {
            begin(3);
            step();
            step();
            assert!(check().is_ok());
            i.cancel();
            check()
        });
        assert_eq!(out, Err(StatError::Cancelled));
        assert_eq!(i.progress(), (2, 3));
        assert!(current().is_none(), "the interrupt is removed afterwards");
    }

    #[test]
    fn interrupts_nest_and_are_restored_after_a_panic() {
        let outer = Interrupt::new();
        with(Arc::clone(&outer), || {
            let inner = Interrupt::new();
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| with(inner, || panic!("boom"))));
            assert!(caught.is_err());
            assert!(Arc::ptr_eq(&current().unwrap(), &outer));
        });
    }
}
