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

    /// try to start a buffer swap
    pub fn try_start_buffer_swap(&mut self) -> Result<(), ValidationErrorOf<StrategyOf<S>>> {
        // SAFETY: DelayedWriter doesn't expose a `&mut Writer` if there is an in progress swap
        self.swap = Some(unsafe { self.writer.try_start_buffer_swap()? });
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

    /// finish an in progress buffer swap
    pub fn finish_swap(&mut self) -> &mut Writer<S> {
        if let Some(swap) = core::mem::take(&mut self.swap) {
            self.writer.finish_swap(swap);
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
        self.swap
            .as_mut()
            .map_or(true, |swap| self.writer.is_swap_finished(swap))
    }
}

impl<S: StrongRef> Deref for DelayedWriter<S> {
    type Target = Writer<S>;

    fn deref(&self) -> &Self::Target {
        &self.writer
    }
}

#[test]
fn test() {
    let mut shared = crate::raw::Shared::new(
        crate::strategy::TrackingStrategy::new(),
        crate::raw::SliceRawDoubleBuffer::from_array([10, 20]),
    );
    let mut writer = DelayedWriter::new(Writer::new(
        &mut shared as &mut crate::raw::Shared<_, crate::raw::SliceRawDoubleBuffer<[_]>>,
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
