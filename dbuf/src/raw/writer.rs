//! the writer to a double buffer

use crate::interface::{
    BufferOf, CaptureOf, IntoStrongRef, RawBuffers, RawBuffersOf, Strategy, StrategyOf, StrongRef,
    ValidationErrorOf, WeakOf, Which, WriterTag,
};

use super::Reader;

/// The writer to a double buffer
pub struct Writer<S, W = WriterTag<StrategyOf<S>>> {
    /// the writer tag which identifies this writer to the strategy
    tag: W,
    /// a strong pointer to the double buffer's shared state
    ptr: S,
}

/// The two buffers
#[non_exhaustive]
#[derive(Debug)]
pub struct Split<'a, T: ?Sized> {
    /// the reader buffer
    pub reader: &'a T,
    /// the writer buffer
    pub writer: &'a T,
}

/// The two buffers
#[non_exhaustive]
#[derive(Debug)]
pub struct SplitMut<'a, T: ?Sized> {
    /// the reader buffer
    pub reader: &'a T,
    /// the writer buffer
    pub writer: &'a mut T,
}

/// The two buffers
pub struct Swap<S: StrongRef> {
    /// the capture token which represents all the readers
    capture: CaptureOf<StrategyOf<S>>,
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

    /// split the writer into the two read-only buffers
    pub fn split(&self) -> Split<'_, BufferOf<RawBuffersOf<S>>> {
        let shared = &*self.ptr;
        // SAFETY: split can't race with `try_start_buffer_swap` because `try_start_buffer_swap`
        // takes `&mut self` which can't be called at the same time as `&self` methods
        let which = unsafe { shared.which.load_unsync() };
        let (writer, reader) = shared.buffers.get(which);

        // SAFETY:
        // * the two pointers are valid for `'_`
        // * we have a `&self` so we can safely access a shared view into the reader buffer
        // * we have a `&self` so we can safely access a shared view into the writer buffer
        unsafe {
            Split {
                reader: &*reader,
                writer: &*writer,
            }
        }
    }

    /// split the writer into the two read-only buffers
    pub fn split_mut(&mut self) -> SplitMut<'_, BufferOf<RawBuffersOf<S>>> {
        let shared = &*self.ptr;
        // SAFETY: split can't race with `try_start_buffer_swap` because `try_start_buffer_swap`
        // takes `&mut self` which can't be called at the same time as `&self` methods
        let which = unsafe { shared.which.load_unsync() };
        let (writer, reader) = shared.buffers.get(which);

        // SAFETY:
        // * the two pointers are valid for `'_`
        // * we have a `&mut self` so we can safely access a shared view into the reader buffer
        // * we have a `&mut self` so we can safely access a exclusive view into the writer buffer (no readers can read this buffer)
        unsafe {
            SplitMut {
                reader: &*reader,
                writer: &mut *writer,
            }
        }
    }

    /// Swap the two buffers
    pub fn try_swap_buffers(&mut self) -> Result<(), ValidationErrorOf<StrategyOf<S>>> {
        // SAFETY: we call `finish_swap`
        let swap = unsafe { self.try_start_buffer_swap()? };
        self.finish_swap(swap);
        Ok(())
    }

    /// Swap the two buffers
    pub fn swap_buffers(&mut self)
    where
        StrategyOf<S>: Strategy<ValidationError = core::convert::Infallible>,
    {
        match self.try_swap_buffers() {
            Ok(()) => (),
            Err(inf) => match inf {},
        }
    }

    /// try to start a buffer swap
    ///
    /// # Safety
    ///
    /// You must either poll `is_swap_finished` or call `finish_swap` with the `swap`
    /// before calling any other methods that take `&mut self`
    pub unsafe fn try_start_buffer_swap(
        &mut self,
    ) -> Result<Swap<S>, ValidationErrorOf<StrategyOf<S>>> {
        let shared = &*self.ptr;
        let validation_token = shared.strategy.validate_swap(&mut self.tag)?;

        shared.which.flip();

        // SAFETY:
        //
        // * Must be called immediately after swapping the buffers
        // * the validation token must have come from a call to `validate_swap` right before swapping the buffers
        let capture = unsafe {
            shared
                .strategy
                .capture_readers(&mut self.tag, validation_token)
        };

        Ok(Swap { capture })
    }

    /// Check if all readers have exited the write buffer
    pub fn is_swap_finished(&self, swap: &mut Swap<S>) -> bool {
        self.ptr
            .strategy
            .have_readers_exited(&self.tag, &mut swap.capture)
    }

    /// Check if all readers have exited the write buffer
    pub fn finish_swap(&self, mut swap: Swap<S>) {
        /// Drop slow to reduce the code size of `finish_swap`
        #[cold]
        #[inline(never)]
        fn drop_slow<T>(_: T) {}

        let mut swap = FinishSwapOnDrop {
            tag: &self.tag,
            shared: &self.ptr,
            capture: &mut swap.capture,
        };

        if !swap.is_finished() {
            drop_slow(FinishSwapOnDrop {
                tag: swap.tag,
                shared: swap.shared,
                capture: swap.capture,
            })
        }

        core::mem::forget(swap);
    }
}

/// A guard to ensure that the swap is finished before exiting `finish_swap`
struct FinishSwapOnDrop<'a, S: Strategy, B: ?Sized> {
    /// the writer associated with this swap
    tag: &'a WriterTag<S>,
    /// the shared buffer for this swap
    shared: &'a super::Shared<S, B>,
    /// the capture token for this swap
    capture: &'a mut CaptureOf<S>,
}

impl<S: Strategy, B: ?Sized> FinishSwapOnDrop<'_, S, B> {
    /// is the given swap finished
    fn is_finished(&mut self) -> bool {
        self.shared
            .strategy
            .have_readers_exited(self.tag, self.capture)
    }
}

impl<S: Strategy, B: ?Sized> Drop for FinishSwapOnDrop<'_, S, B> {
    fn drop(&mut self) {
        while !self.is_finished() {
            self.shared.strategy.pause(self.tag)
        }
    }
}
