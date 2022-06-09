//! An operation log which tracks which operations were applied to which buffer

use std::vec::Vec;

/// An operation that can be applied to a buffer
pub trait Operation<B: ?Sized>: Sized {
    /// apply this operation to the buffer
    fn apply(&mut self, buffer: &mut B);

    /// apply this operation to the buffer for the last time
    fn apply_last(mut self, buffer: &mut B) {
        self.apply(buffer)
    }
}

/// an operation log which tracks which operations were applied to which buffer
pub struct OpLog<O> {
    /// the list of in progress operations
    ops: Vec<O>,
    /// the number of operations that have been applied to the previous buffer
    applied: usize,
}

impl<O> OpLog<O> {
    /// create a new op log
    pub const fn new() -> Self {
        Self::from_vec(Vec::new())
    }

    /// create a new op log
    pub const fn from_vec(ops: Vec<O>) -> Self {
        Self { ops, applied: 0 }
    }

    /// Reserves capacity for at least `additional` more elements to be inserted in a given `OpLog`
    pub fn reserve(&mut self, additional: usize) {
        self.ops.reserve(additional)
    }

    /// Appends an element to the back of the `OpLog`.
    pub fn push(&mut self, op: O) {
        self.ops.push(op)
    }

    /// apply all operations to the given buffer
    pub fn apply<B: ?Sized>(&mut self, buffer: &mut B)
    where
        O: Operation<B>,
    {
        for op in self.ops.drain(..self.applied) {
            op.apply_last(buffer);
        }

        self.applied = self.ops.len();

        for op in self.ops.iter_mut() {
            op.apply(buffer)
        }
    }
}

impl<O> Default for OpLog<O> {
    fn default() -> Self {
        Self::new()
    }
}
