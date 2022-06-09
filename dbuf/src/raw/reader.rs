//! a reader to a double buffer

use core::{marker::PhantomData, mem::ManuallyDrop, ops::Deref, ptr::NonNull};

use crate::interface::{
    BufferOf, RawBuffers, RawBuffersOf, ReaderGuardOf, ReaderTagOf, Strategy, StrategyOf, StrongOf,
    StrongRef, WeakRef, Which,
};

/// A reader to a double buffer
pub struct Reader<W, R = ReaderTagOf<StrategyOf<StrongOf<W>>>> {
    /// the reader tag which identifies this reader to the strategy
    tag: R,
    /// a weak pointer to the double buffer's shared state
    ptr: W,
}

/// A RAII guard which locks the double buffer and allows reading into it
pub struct ReadGuard<'a, S: StrongRef, B: ?Sized = BufferOf<RawBuffersOf<S>>> {
    /// The buffer we're reading into
    buffer: SharedRef<B>,
    /// the raw read guard which locks the double buffer
    /// only used in `Drop`
    _raw: RawReadGuard<'a, S>,
}

/// A RAII guard which locks the double buffer and allows reading into it
#[repr(transparent)]
pub struct SharedRef<B: ?Sized> {
    /// The buffer we're reading into
    ptr: NonNull<B>,
}

// SAFETY: the shared ref is only allows access to &B
unsafe impl<B: ?Sized + Sync> Send for SharedRef<B> {}
// SAFETY: the shared ref is only allows access to &B
unsafe impl<B: ?Sized + Sync> Sync for SharedRef<B> {}

impl<S: StrongRef, B> Deref for ReadGuard<'_, S, B> {
    type Target = B;

    fn deref(&self) -> &Self::Target {
        // SAFETY: the raw guard ensure that the writer can't write to this buffer
        unsafe { self.buffer.ptr.as_ref() }
    }
}
/// A raw RAII guard which specifies how long the reader locks the double buffer for
struct RawReadGuard<'a, S: StrongRef> {
    /// the reader which owns the lock
    tag: &'a mut ReaderTagOf<StrategyOf<S>>,
    /// a strong ref to the shared state to keep it alive
    strong_ref: S,
    /// the reader guard token which the strategy can use to track which readers reading
    guard: ManuallyDrop<ReaderGuardOf<StrategyOf<S>>>,
    /// a lifetime to ensure that no other reads happen at the same time
    lifetime: PhantomData<&'a S>,
}

impl<S: StrongRef> Drop for RawReadGuard<'_, S> {
    fn drop(&mut self) {
        // SAFETY: the guard is created in `Reader::try_get` and never touched until here so it's still valid
        let guard = unsafe { ManuallyDrop::take(&mut self.guard) };
        // SAFETY: the reader (self.tag) was the one that created the guard by construction of `Self`
        unsafe { self.strong_ref.strategy.end_read_guard(self.tag, guard) }
    }
}

impl<W: WeakRef> Reader<W> {
    /// Create a new reader from a tag and ptr
    ///
    /// # Safety
    ///
    /// If the ptr is dangling (i.e. if `W::upgrade` would return `None`) the reader tag may dangle
    /// If the ptr is not dangling (i.e. if `W::upgrade` would return `Some`) the reader tag must be managed by the strategy
    pub unsafe fn from_raw_parts(tag: ReaderTagOf<StrategyOf<StrongOf<W>>>, ptr: W) -> Self {
        Self { tag, ptr }
    }

    /// get a read lock on the double buffer
    pub fn try_get(&mut self) -> Result<ReadGuard<'_, StrongOf<W>>, W::UpgradeError> {
        let strong_ref: W::Strong = W::upgrade(&self.ptr)?;
        let shared = &*strong_ref;

        // first begin the guard *before* loading which buffer is for reads
        // to avoid racing with the writer
        let guard = shared.strategy.begin_read_guard(&mut self.tag);

        let which = shared.which.load();
        let (_writer, reader) = shared.buffers.get(which);
        Ok(ReadGuard {
            buffer: SharedRef {
                // SAFETY: the reader ptr is valid for as long as the `strong_ref` is alive
                ptr: unsafe { NonNull::new_unchecked(reader as *mut _) },
            },
            _raw: RawReadGuard {
                tag: &mut self.tag,
                strong_ref,
                guard: ManuallyDrop::new(guard),
                lifetime: PhantomData,
            },
        })
    }

    /// get a read lock on the double buffer
    pub fn get(&mut self) -> ReadGuard<'_, StrongOf<W>>
    where
        W: WeakRef<UpgradeError = core::convert::Infallible>,
    {
        match self.try_get() {
            Ok(guard) => guard,
            Err(inf) => match inf {},
        }
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
