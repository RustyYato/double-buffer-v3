//! A delayed writer which allowed you to safely start a swap

use std::ops::Deref;

use crate::{
    interface::{CaptureOf, Strategy, StrategyOf, StrongRef, ValidationErrorOf, WriterTag},
    raw::{Swap, Writer},
};

/// A delayed writer which allows safely starting swaps
pub struct DelayedWriter<S, W = WriterTag<StrategyOf<S>>, C = CaptureOf<StrategyOf<S>>> {
    /// the underlying writer
    writer: Writer<S, W>,
    /// a potentially in-progress swap
    swap: Option<Swap<C>>,
}

impl<S: StrongRef> From<Writer<S>> for DelayedWriter<S> {
    fn from(writer: Writer<S>) -> Self {
        Self::new(writer)
    }
}

impl<S: StrongRef> DelayedWriter<S> {
    /// create a new delayed writer
    pub const fn new(writer: Writer<S>) -> Self {
        DelayedWriter { writer, swap: None }
    }

    /// try to swap the buffers
    pub fn try_swap_buffers(&mut self) -> Result<&mut Writer<S>, ValidationErrorOf<StrategyOf<S>>> {
        self.finish_swap();
        self.try_start_buffer_swap()?;
        Ok(self.finish_swap())
    }

    /// swap the buffers
    pub fn swap_buffers(&mut self) -> &mut Writer<S>
    where
        StrategyOf<S>: Strategy<ValidationError = core::convert::Infallible>,
    {
        self.finish_swap();
        self.start_buffer_swap();
        self.finish_swap()
    }

    /// try to start a buffer swap
    pub fn try_start_buffer_swap(&mut self) -> Result<(), ValidationErrorOf<StrategyOf<S>>> {
        if self.swap.is_some() {
            return Ok(());
        }

        // SAFETY: DelayedWriter doesn't expose a `&mut Writer` if there is an in progress swap
        let swap = unsafe { self.writer.try_start_buffer_swap()? };

        // SAFETY: it's always safe to write to a `&mut _`
        unsafe { core::ptr::write(&mut self.swap, Some(swap)) };

        Ok(())
    }

    /// start a buffer swap
    pub fn start_buffer_swap(&mut self)
    where
        StrategyOf<S>: Strategy<ValidationError = core::convert::Infallible>,
    {
        match self.try_start_buffer_swap() {
            Ok(_) => (),
            Err(inf) => match inf {},
        }
    }

    /// get a mutable reference to the inner writer if the swap is finished
    pub fn try_writer_mut(&mut self) -> Option<&mut Writer<S>> {
        if self.is_swap_finished() {
            Some(&mut self.writer)
        } else {
            None
        }
    }

    /// finish an in progress buffer swap
    pub fn finish_swap(&mut self) -> &mut Writer<S> {
        if let Some(ref mut swap) = self.swap {
            // SAFETY: this writer created the swap
            unsafe { self.writer.finish_swap(swap) }
            self.swap = None;
        }

        &mut self.writer
    }

    /// finish an in progress buffer swap
    pub fn into_finish_swap(mut self) -> Writer<S> {
        self.finish_swap();

        self.writer
    }

    /// check if the swap is finished
    pub fn is_swap_finished(&mut self) -> bool {
        match self.swap.as_mut() {
            None => true,
            Some(swap) => {
                // SAFETY: this writer created the swap
                if unsafe { self.writer.is_swap_finished(swap) } {
                    self.swap = None;
                    true
                } else {
                    false
                }
            }
        }
    }
}

impl<S: StrongRef> Deref for DelayedWriter<S> {
    type Target = Writer<S>;

    fn deref(&self) -> &Self::Target {
        &self.writer
    }
}

#[test]
#[cfg_attr(feature = "loom", ignore = "when using loom: ignore normal tests")]
fn test() {
    let mut shared = crate::raw::Shared::from_raw_parts(
        crate::strategy::TrackingStrategy::new(),
        crate::raw::SliceRawDbuf::from_array([10, 20]),
    );
    let mut writer = DelayedWriter::new(Writer::new(
        &mut shared as &mut crate::raw::Shared<_, crate::raw::SliceRawDbuf<[_]>>,
    ));

    let split = writer.split();
    assert_eq!(split.writer, [10]);
    assert_eq!(split.reader, [20]);

    let mut r1 = writer.reader();
    let mut r2 = writer.reader();

    let a = r1.get();

    writer.start_buffer_swap();

    let b = r2.get();

    assert!(!core::ptr::eq(&*a, &*b));

    assert!(!writer.is_swap_finished());

    drop(a);

    writer.into_finish_swap();
}
