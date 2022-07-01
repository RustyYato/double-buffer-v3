#[allow(clippy::missing_docs_in_private_items)]
#[cfg(feature = "std")]
// #[cfg(FALSE)]
mod imp {
    pub use std::thread::ThreadId;

    pub fn current() -> ThreadId {
        std::thread::current().id()
    }
}

#[allow(clippy::missing_docs_in_private_items)]
#[cfg(not(feature = "std"))]
mod imp {
    #[derive(Debug, PartialEq, Eq, Hash)]
    pub struct ThreadId;

    pub fn current() -> ThreadId {
        ThreadId
    }
}

/// A unique token which specifies the identity of the thread
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ThreadId(imp::ThreadId);

impl ThreadId {
    /// Produce the current thread's unique identifier
    pub fn current() -> Self {
        Self(imp::current())
    }
}
