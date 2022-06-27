//! An operation log which tracks which operations were applied to which buffer
//!
//! An [`OpLog`] can be used to keep the two halves of the double buffer in sync
//! by applying all operations to both halves. It does this via the [`Operation`] trait,
//! the first time an operation is applied, [`OpLog`] will call [`Operation::apply`],
//! the second time it will call [`Operation::apply_last`]. (The distinction allows for more
//! optimized implementation of [`apply_last`](Operation::apply_last)).
//!
//! For example, consider a double buffered hash map.
//!
//! ```ignore
//! let MAP = Map::new():
//!
//! MAP.insert(...);
//! MAP.insert(...);
//! MAP.insert(...);
//! MAP.insert(...);
//! MAP.update(...);
//! MAP.remove(...);
//! ```
//!
//! Each of the methods above could be an operation in the [`OpLog`] to insert/update/remove
//! elements in the map. Then when the operations are all [applied](OpLog::apply) (e.g. when
//! the buffers are being swapped). Then we apply the operations once to the back buffer,
//! then bring the back buffer forward. Later when calling [`OpLog::apply`](OpLog::apply),
//! the [`OpLog`] can apply the operations again to the other buffer.
//!
//! ### Panics
//!
//! If an operation panics, then subsequent operations may be skipped or dropped. This is to allow for
//! more optimized operation application during non-panic situations, but may make other double buffered
//! data structures built atop this out of sync! So be careful to not panic during operation application.

use std::vec::Vec;

/// An operation that can be applied to a buffer
///
/// see
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

    /// Shrinks the capacity of the vector with a lower bound.
    ///
    /// The capacity will remain at least as large as both the length
    /// and the supplied value.
    ///
    /// If the current capacity is less than the lower limit, this is a no-op.
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.ops.shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the vector as much as possible.
    ///
    /// It will drop down as close as possible to the length but the allocator
    /// may still inform the vector that there is space for a few more elements.
    pub fn shrink_to_fit(&mut self) {
        self.ops.shrink_to_fit()
    }

    /// Reserves capacity for at least `additional` more elements to be inserted in a given `OpLog`
    pub fn reserve(&mut self, additional: usize) {
        self.ops.reserve(additional)
    }

    /// Appends an element to the back of the `OpLog`.
    pub fn push(&mut self, op: O) {
        self.ops.push(op)
    }

    /// All operations which haven't yet been applied
    pub fn unapplied(&self) -> &[O] {
        &self.ops[self.applied..]
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
