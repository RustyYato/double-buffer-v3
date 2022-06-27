//! An operation based writer

use std::{convert::Infallible, ops::Deref};

use crate::{
    delayed::DelayedWriter,
    interface::{BufferOf, CaptureOf, RawBuffersOf, Strategy, StrategyOf, StrongRef, WriterTag},
    op_log::{OpLog, Operation},
    raw::Writer,
};

/// An operation based writer
pub struct OpWriter<S, O, W = WriterTag<StrategyOf<S>>, C = CaptureOf<StrategyOf<S>>> {
    /// the underlying writer
    writer: DelayedWriter<S, W, C>,
    /// the operation log
    op_log: OpLog<O>,
}

impl<S: StrongRef, O> From<DelayedWriter<S>> for OpWriter<S, O> {
    fn from(writer: DelayedWriter<S>) -> Self {
        Self::from_raw_parts(writer, OpLog::new())
    }
}

impl<S: StrongRef, O> From<Writer<S>> for OpWriter<S, O> {
    fn from(writer: Writer<S>) -> Self {
        Self::from_raw_parts(writer.into(), OpLog::new())
    }
}

impl<S: StrongRef, O> OpWriter<S, O> {
    /// create an op writer from raw parts
    pub const fn from_raw_parts(writer: DelayedWriter<S>, op_log: OpLog<O>) -> Self {
        Self { writer, op_log }
    }

    /// deconstruct the op writer into it's raw parts
    pub fn into_raw_parts(self) -> (DelayedWriter<S>, OpLog<O>) {
        (self.writer, self.op_log)
    }

    /// All operations which haven't yet been applied
    pub fn unapplied(&self) -> &[O] {
        self.op_log.unapplied()
    }

    /// Shrinks the capacity of the vector with a lower bound.
    ///
    /// The capacity will remain at least as large as both the length
    /// and the supplied value.
    ///
    /// If the current capacity is less than the lower limit, this is a no-op.
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.op_log.shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the vector as much as possible.
    ///
    /// It will drop down as close as possible to the length but the allocator
    /// may still inform the vector that there is space for a few more elements.
    pub fn shrink_to_fit(&mut self) {
        self.op_log.shrink_to_fit()
    }

    /// Reserves capacity for at least `additional` more elements to be inserted in a given `OpWriter`
    pub fn reserve(&mut self, additional: usize) {
        self.op_log.reserve(additional)
    }
}

impl<S: StrongRef, O: Operation<BufferOf<RawBuffersOf<S>>>> OpWriter<S, O>
where
    StrategyOf<S>: Strategy<ValidationError = Infallible>,
{
    /// apply an operation to the op writer
    pub fn apply(&mut self, op: O) {
        self.op_log.push(op)
    }

    /// swap buffers if there are some unapplied operations
    pub fn flush(&mut self) {
        if !self.unapplied().is_empty() {
            self.swap_buffers();
        }
    }

    /// swap the underlying buffers and apply any unapplied operations
    pub fn swap_buffers(&mut self) {
        let writer = self.writer.finish_swap();
        let writer = writer.split_mut().writer;
        self.op_log.apply(writer);
        self.writer.start_buffer_swap();
    }
}

impl<S: StrongRef, O> Deref for OpWriter<S, O> {
    type Target = Writer<S>;

    fn deref(&self) -> &Self::Target {
        &self.writer
    }
}
