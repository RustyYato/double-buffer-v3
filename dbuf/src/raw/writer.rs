//! the writer to a double buffer

use crate::interface::{
    Buffer, CaptureOf, IntoStrongRef, RawBuffers, RawBuffersOf, Strategy, StrategyOf, StrongRef,
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
    pub fn split(&self) -> Split<'_, Buffer<RawBuffersOf<S>>> {
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
    pub fn split_mut(&mut self) -> SplitMut<'_, Buffer<RawBuffersOf<S>>> {
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
        // SAFETY: FIXME
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
    /// FIXME
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
        if self.is_swap_finished(&mut swap) {
            return;
        }

        self.finish_swap_slow(swap)
    }

    /// Check if all readers have exited the write buffer
    fn finish_swap_slow(&self, mut swap: Swap<S>) {
        while !self.is_swap_finished(&mut swap) {
            self.ptr.strategy.pause(&self.tag)
        }
    }
}
