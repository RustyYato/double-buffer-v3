//! various waiting strategeis

use crate::interface::WaitStrategy;

#[derive(Default)]
/// This waiter will do nothing on wait
pub struct NoopWait;

impl WaitStrategy for NoopWait {
    type State = ();

    fn wait(&self, (): &mut Self::State) -> bool {
        true
    }

    fn notify(&self) {}
}

#[derive(Default)]
/// This waiter will spin using exponential backoff
pub struct SpinWait;

impl WaitStrategy for SpinWait {
    type State = u32;

    fn wait(&self, counter: &mut Self::State) -> bool {
        let count = *counter;
        *counter = count.wrapping_add(1).max(10);

        for _ in 0..1 << count {
            core::hint::spin_loop()
        }

        count == 10
    }

    fn notify(&self) {}
}

/// This waiter will park the thread on wait
#[cfg(feature = "std")]
pub struct ThreadParker {
    ///
    mutex: std::sync::Mutex<()>,
    ///
    cv: std::sync::Condvar,
}

#[cfg(feature = "std")]
impl ThreadParker {
    /// Create a new thread parker
    pub fn new() -> Self {
        Self {
            mutex: std::sync::Mutex::new(()),
            cv: std::sync::Condvar::new(),
        }
    }
}

#[cfg(feature = "std")]
impl Default for ThreadParker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl WaitStrategy for ThreadParker {
    type State = ();

    #[cold]
    fn wait(&self, _: &mut Self::State) -> bool {
        let lock = self
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        #[allow(clippy::let_underscore_lock)]
        let _ = self.cv.wait(lock);

        true
    }

    fn notify(&self) {
        self.cv.notify_one();
    }
}

/// This waiter will spin for using exponential backoff, then park the thread
#[cfg(feature = "std")]
pub struct AdaptiveWait {
    /// the thread parker
    thread: ThreadParker,
}

#[cfg(feature = "std")]
impl AdaptiveWait {
    /// create a new adaptive waiter
    pub fn new() -> Self {
        Self {
            thread: ThreadParker::new(),
        }
    }
}

#[cfg(feature = "std")]
impl Default for AdaptiveWait {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl WaitStrategy for AdaptiveWait {
    type State = u32;

    #[cold]
    fn wait(&self, counter: &mut Self::State) -> bool {
        if SpinWait.wait(counter) {
            self.thread.wait(&mut ());

            true
        } else {
            false
        }
    }

    fn notify(&self) {
        self.thread.notify();
    }
}

/// This waiter will spin for using exponential backoff, then park the thread
///
/// This behavior is subject to change
pub struct DefaultWait {
    /// the inner parker type
    #[cfg(feature = "std")]
    adaptive: AdaptiveWait,
}

impl DefaultWait {
    /// create a new default waiter
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "std")]
            adaptive: AdaptiveWait::new(),
        }
    }
}

#[cfg(feature = "std")]
impl Default for DefaultWait {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl WaitStrategy for DefaultWait {
    type State = u32;

    #[inline]
    fn wait(&self, counter: &mut Self::State) -> bool {
        #[cfg(not(feature = "std"))]
        SpinWait.park(counter);
        #[cfg(feature = "std")]
        self.adaptive.wait(counter)
    }

    fn notify(&self) {
        #[cfg(feature = "std")]
        self.adaptive.notify();
    }
}
