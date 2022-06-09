//! An operation based writer

use std::convert::Infallible;

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
}

impl<S: StrongRef, O: Operation<BufferOf<RawBuffersOf<S>>>> OpWriter<S, O> {
    /// apply an operation to the op writer
    pub fn apply(&mut self, op: O) {
        self.op_log.push(op)
    }

    /// Reserves capacity for at least `additional` more elements to be inserted in a given `OpWriter`
    pub fn reserve(&mut self, additional: usize) {
        self.op_log.reserve(additional)
    }

    /// apply an operation to the op writer
    pub fn swap_buffers(&mut self)
    where
        StrategyOf<S>: Strategy<ValidationError = Infallible>,
    {
        let writer = self.writer.finish_swap();
        let writer = writer.split_mut().writer;
        self.op_log.apply(writer);
        self.writer.start_buffer_swap();
    }
}
