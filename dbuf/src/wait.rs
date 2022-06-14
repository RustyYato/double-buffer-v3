//! various waiting strategeis

use crate::interface::WaitStrategy;

#[derive(Default)]
/// This parker will do nothing on park
pub struct NoopWait;

impl WaitStrategy for NoopWait {
    type State = ();

    fn wait(&self, (): &mut Self::State) -> bool {
        true
    }

    fn notify(&self) {}
}

#[derive(Default)]
/// This parker will do nothing on park
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

/// This parker will do nothing on park
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

/// This parker will do nothing on park
#[cfg(feature = "std")]
pub struct AdaptiveWait {
    ///
    thread: ThreadParker,
}

#[cfg(feature = "std")]
impl AdaptiveWait {
    /// create a new adaptive parker
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

/// This parker will do nothing on park
pub struct DefaultWait {
    /// the inner parker type
    #[cfg(feature = "std")]
    adaptive: AdaptiveWait,
}

impl DefaultWait {
    /// create a new default parker
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
