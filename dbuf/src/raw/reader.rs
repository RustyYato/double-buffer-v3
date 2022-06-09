//! a reader to a double buffer

use crate::interface::{ReaderTag, Strategy, StrategyOf, StrongOf, WeakRef};

/// A reader to a double buffer
pub struct Reader<W, R = ReaderTag<StrategyOf<StrongOf<W>>>> {
    /// the reader tag which identifies this reader to the strategy
    tag: R,
    /// a weak pointer to the double buffer's shared state
    ptr: W,
}

impl<W: WeakRef> Reader<W> {
    /// Create a new reader from a tag and ptr
    ///
    /// # Safety
    ///
    /// If the ptr is dangling (i.e. if `W::upgrade` would return `None`) the reader tag may dangle
    /// If the ptr is not dangling (i.e. if `W::upgrade` would return `Some`) the reader tag must be managed by the strategy
    pub unsafe fn from_raw_parts(tag: ReaderTag<StrategyOf<StrongOf<W>>>, ptr: W) -> Self {
        Self { tag, ptr }
    }
}

impl<W: WeakRef> Clone for Reader<W> {
    fn clone(&self) -> Self {
        match W::upgrade(&self.ptr) {
            Ok(ptr) => {
                // Safety: the writer is owned by this strategy as it was created by this strategy
                let tag = unsafe { ptr.strategy.create_reader_tag_from_reader(&self.tag) };
                // Safety: the writer is owned by this strategy as it was created by this strategy
                unsafe { Self::from_raw_parts(tag, self.ptr.clone()) }
            }
            Err(_) => {
                let tag = <StrategyOf<StrongOf<W>> as Strategy>::dangling_reader_tag();
                // Safety: this reader tag will never be used because the writer is dead
                unsafe { Self::from_raw_parts(tag, self.ptr.clone()) }
            }
        }
    }
}
