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
pub struct Swap<C> {
    /// the capture token which represents all the readers
    capture: C,
}

impl<S: StrongRef> Writer<S> {
    /// Create a new writer to the double buffer
    pub fn new<T: IntoStrongRef<Strong = S>>(mut ptr: T) -> Self {
        // Safety: we just created a strong ref, so this is the first time create writer tag is called
        let tag = unsafe { ptr.get_mut().strategy.create_writer_tag() };
        let ptr = ptr.into_strong();
        Self { tag, ptr }
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

        // SAFETY: this swap was just created by this writer which means
        // it was created by this strategy with this writer tag.
        unsafe {
            let mut guard =
                scopeguard::guard((self, swap), |(this, mut swap)| this.finish_swap(&mut swap));
            let (this, swap) = &mut *guard;

            this.finish_swap(swap)
        };
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
    /// You must either poll `is_swap_finished` until it returns true or
    /// call `finish_swap` with the `swap` before calling any other methods
    /// that take `&mut self`
    pub unsafe fn try_start_buffer_swap(
        &mut self,
    ) -> Result<Swap<CaptureOf<StrategyOf<S>>>, ValidationErrorOf<StrategyOf<S>>> {
        let shared = &*self.ptr;
        let validation_token = shared.strategy.validate_swap(&mut self.tag)?;

        shared.which.flip();

        // SAFETY:
        //
        // * The validation token must have come from a call to `validate_swap` right before swapping the buffers
        //      * we flip the buffers in between calling `validate_swap` and `capture_readers`
        // * Must poll `have_readers_exited` until it returns true before calling `validate_swap` again
        //      * guarnteed by caller
        let capture = unsafe {
            shared
                .strategy
                .capture_readers(&mut self.tag, validation_token)
        };

        Ok(Swap { capture })
    }

    /// Check if all readers have exited the write buffer
    ///
    /// # Safety
    ///
    /// the swap should have been created by `self`
    pub unsafe fn is_swap_finished(&self, swap: &mut Swap<CaptureOf<StrategyOf<S>>>) -> bool {
        // SAFETY: this swap was created by this writer which means
        // it was created by this strategy with this writer tag.
        unsafe {
            self.ptr
                .strategy
                .have_readers_exited(&self.tag, &mut swap.capture)
        }
    }

    /// Check if all readers have exited the write buffer
    ///
    /// # Safety
    ///
    /// the swap should have been created by `self`
    pub unsafe fn finish_swap(&self, swap: &mut Swap<CaptureOf<StrategyOf<S>>>) {
        // SAFETY: guaranteed by caller
        if !unsafe { self.is_swap_finished(swap) } {
            self.finish_swap_slow(swap)
        }
    }

    #[cold]
    #[inline(never)]
    /// Drop slow to reduce the code size of `finish_swap`
    fn finish_swap_slow(&self, swap: &mut Swap<CaptureOf<StrategyOf<S>>>) {
        let mut pause = Default::default();
        // SAFETY: guaranteed by caller
        while !unsafe { self.is_swap_finished(swap) } {
            self.ptr.strategy.pause(&self.tag, &mut pause)
        }
    }
}
