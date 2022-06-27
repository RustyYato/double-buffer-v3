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

impl<S: StrongRef, B: ?Sized> Deref for ReadGuard<'_, S, B> {
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
    strong_ref: Result<S, &'a StrategyOf<S>>,
    /// the reader guard token which the strategy can use to track which readers reading
    guard: ManuallyDrop<ReaderGuardOf<StrategyOf<S>>>,
    /// a lifetime to ensure that no other reads happen at the same time
    lifetime: PhantomData<&'a S>,
}

impl<S: StrongRef> Drop for RawReadGuard<'_, S> {
    fn drop(&mut self) {
        // SAFETY: the guard is created in `Reader::try_get` and never touched until here so it's still valid
        let guard = unsafe { ManuallyDrop::take(&mut self.guard) };

        let strategy = match self.strong_ref {
            Ok(ref strong_ref) => &strong_ref.strategy,
            Err(strategy) => strategy,
        };

        // SAFETY: the reader (self.tag) was the one that created the guard by construction of `Self`
        unsafe { strategy.end_read_guard(self.tag, guard) }
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
        let strong_ref;
        let shared = match self.ptr.as_ref() {
            Some(shared) => {
                strong_ref = Err(&shared.strategy);
                shared
            }
            _ => {
                strong_ref = Ok(W::upgrade(&self.ptr)?);
                strong_ref.as_ref().ok().unwrap()
            }
        };

        // first begin the guard *before* loading which buffer is for reads
        // to avoid racing with the writer
        //
        // SAFETY: the upgrade succeeded so the reader tag isn't dangling
        let guard = unsafe { shared.strategy.begin_read_guard(&mut self.tag) };

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

    /// Clones the reader without attemping to upgrade the pointer
    pub fn copy_tag(&self) -> Self
    where
        W: Clone,
        ReaderTagOf<StrategyOf<StrongOf<W>>>: Copy,
    {
        Self {
            tag: self.tag,
            ptr: self.ptr.clone(),
        }
    }
}

impl<W: WeakRef> Copy for Reader<W>
where
    W: Copy,
    ReaderTagOf<StrategyOf<StrongOf<W>>>: Copy,
{
}
impl<W: WeakRef> Clone for Reader<W> {
    fn clone(&self) -> Self {
        if <StrategyOf<StrongOf<W>> as Strategy>::READER_TAG_NEEDS_CONSTRUCTION {
            if let Ok(ptr) = W::upgrade(&self.ptr) {
                // Safety: the writer is owned by this strategy as it was created by this strategy
                let tag = unsafe { ptr.strategy.create_reader_tag_from_reader(&self.tag) };
                // Safety: the writer is owned by this strategy as it was created by this strategy
                return unsafe { Self::from_raw_parts(tag, self.ptr.clone()) };
            }
        }

        let tag = <StrategyOf<StrongOf<W>> as Strategy>::dangling_reader_tag();
        // Safety: this reader tag will never be used because the writer is dead
        unsafe { Self::from_raw_parts(tag, self.ptr.clone()) }
    }
}

impl<'a, S: StrongRef, B: ?Sized> ReadGuard<'a, S, B> {
    /// Map the contained type
    pub fn map<T: ?Sized>(self, f: impl FnOnce(&B) -> &T) -> ReadGuard<'a, S, T> {
        // SAFETY: the raw guard ensure that the writer can't write to this buffer
        let ptr = f(unsafe { self.buffer.ptr.as_ref() });

        ReadGuard {
            buffer: SharedRef {
                ptr: NonNull::from(ptr),
            },
            _raw: self._raw,
        }
    }

    /// Map the contained type
    pub fn try_map<T: ?Sized>(
        self,
        f: impl FnOnce(&B) -> Option<&T>,
    ) -> Result<ReadGuard<'a, S, T>, Self> {
        // SAFETY: the raw guard ensure that the writer can't write to this buffer
        if let Some(ptr) = f(unsafe { self.buffer.ptr.as_ref() }) {
            Ok(ReadGuard {
                buffer: SharedRef {
                    ptr: NonNull::from(ptr),
                },
                _raw: self._raw,
            })
        } else {
            Err(self)
        }
    }
}
