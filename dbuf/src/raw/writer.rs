//! the writer to a double buffer

use crate::interface::{IntoStrongRef, Strategy, StrategyOf, StrongRef, WeakOf, WriterTag};

use super::Reader;

/// The writer to a double buffer
pub struct Writer<S, W = WriterTag<StrategyOf<S>>> {
    /// the writer tag which identifies this writer to the strategy
    tag: W,
    /// a strong pointer to the double buffer's shared state
    ptr: S,
}

impl<S: StrongRef> Writer<S> {
    /// Create a new writer to the double buffer
    pub fn new<T: IntoStrongRef<Strong = S>>(ptr: T) -> Self {
        let ptr = ptr.into_strong();
        let shared = &*ptr;
        Self {
            /// Safety: we just created a strong ref, so this is the first time
            /// create writer tag is called
            tag: unsafe { shared.strategy.create_writer_tag() },
            ptr,
        }
    }

    /// Create a new reader to the double buffer
    pub fn reader(&self) -> Reader<WeakOf<S>> {
        // Safety: the writer is owned by this strategy as it was created by this strategy
        let tag = unsafe { self.ptr.strategy.create_reader_tag_from_writer(&self.tag) };
        // Safety: the reader tag is owned by this strategy as it was created by this strategy
        unsafe { Reader::from_raw_parts(tag, S::downgrade(&self.ptr)) }
    }
}
