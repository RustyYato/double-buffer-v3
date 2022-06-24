//! A hazard pointer strategy
//!
//! ## Basic overview:
//!
//! The [`LocalHazardStrategy`] keeps track of the current generation, which
//! let's us know when the reader acquired a read guard (more on this later).
//! Each time we swap the buffers, we increment the generation and capture
//! the current sub-sequence of readers that are in the previous generation.
//!
//! We then iterate over this sub-sequence and remove readers as they exit the buffer.
//!
//! Once all readers have been removed, then we have finished the swap.
//!
//! ## Implementation Details
//!
//! The key data structure is `ActiveReader` reproduced here
//!
//! ```
//! # use core::cell::Cell;
//! struct ActiveReader {
//!     next: *mut ActiveReader,
//!     next_captured: *mut ActiveReader,
//!     generation: Cell<u32>,
//! }
//! ```
//!
//! This structure is a node in two linked lists.
//!
//! * The linked list when you follow the `next` pointer recursively is the entire linked list.
//! * The linked list when you follow the `next_captured` pointer recursively is a sub-sequence of nodes which had
//!     the previous generation when `capture_readers` was called.
//! * `generation` represents both the generation it started reading on
//!
//! Upon insertion into the list, the `next` pointer is immutable and available for reads by the reader.
//! The `next_captured` pointer is owned by the writer, and only the writer may access it.
//! The `generation` value is mutable and read/writable by readers and readably by writers.
//!
//! `generation` is either `0` or holds the `generation`. Where generation is guranteed
//! to be an odd number.
//! If it's `0` then the link is considered `EMPTY`.
//!
//! ### Reads
//!
//! When a reader tries to acquire a read guard, it will find the first non-empty node and put itself there.
//! Then the `ReaderGuard` will point to that node, so when the guard ends, it can simply empty out the node
//! without iterating over the list.
//!
//! ### Swaps
//!
//! When the writer wants to swap
//! * the [`LocalHazardStrategy`] will increment the generation counter (by 2 to stay odd)
//! * the writer will swap the buffers
//! * the [`LocalHazardStrategy`] iterate over the entire list and setup the `next_captured` sub-sequence of
//! readers which are still in the previous generation.
//! * while this subsequence is non-empty the [`LocalHazardStrategy`] will iterate over the sub-sequence and remove
//! elements from the sub-sequence which have are `EMPTY` or not in the same generation.

use core::{cell::Cell, ptr};
use std::boxed::Box;

use crate::interface::Strategy;

/// A hazard pointer strategy
///
/// a lock-free synchronization strategy
///
/// see module level docs for details
pub struct LocalHazardStrategy {
    /// the head of the append-only linked list of possibly active readers
    ptr: Cell<*mut ActiveReader>,
    /// the current generation
    generation: Cell<u32>,
}

/// a link in the linked list of possibly active readers
struct ActiveReader {
    /// the next link in the list
    ///
    /// iterating these links will yield the entire linked list.
    ///
    /// if null => no next link
    /// if non-null => the next link
    next: *mut ActiveReader,

    /// the next link in the captured list
    ///
    /// iterating these links will yield a sub-sequence of the entire linked list
    /// which holds only the nodes which are in the previous generation from the last
    /// `capture_readers`.
    ///
    /// this field is owned by the writer, readers may not touch it
    ///
    /// if null => no next link
    /// if non-null => the next link
    next_captured: *mut ActiveReader,

    /// the generation that this active reader was acquried (or 0 of there isn't an active reader)
    generation: Cell<u32>,
}

impl LocalHazardStrategy {
    /// Create a new hazard strategy
    pub const fn new() -> Self {
        Self {
            ptr: Cell::new(ptr::null_mut()),
            generation: Cell::new(1),
        }
    }

    /// create a new reader tag
    fn create_reader(&self) -> ReaderTag {
        ReaderTag(())
    }
}

impl Default for LocalHazardStrategy {
    fn default() -> Self {
        Self::new()
    }
}

/// the writer tag for [`LocalHazardStrategy`]
pub struct WriterTag(());
/// the reader tag for [`LocalHazardStrategy`]
#[derive(Clone, Copy)]
pub struct ReaderTag(());
/// the validation token for [`LocalHazardStrategy`]
pub struct ValidationToken(());
/// the capture token for [`LocalHazardStrategy`]
pub struct Capture {
    /// the captured generation
    generation: u32,
    /// the latest active reader for that generation
    start: *mut ActiveReader,
}
/// the reader guard for [`LocalHazardStrategy`]
pub struct ReaderGuard(*mut ActiveReader);

// SAFETY: FIXME
unsafe impl Strategy for LocalHazardStrategy {
    type WriterTag = WriterTag;
    type ReaderTag = ReaderTag;
    type Which = crate::raw::Flag;
    type ValidationToken = ValidationToken;
    type ValidationError = core::convert::Infallible;
    type Capture = Capture;
    type ReaderGuard = ReaderGuard;
    type Pause = ();

    #[inline]
    unsafe fn create_writer_tag(&mut self) -> Self::WriterTag {
        WriterTag(())
    }

    #[inline]
    unsafe fn create_reader_tag_from_writer(&self, _parent: &Self::WriterTag) -> Self::ReaderTag {
        self.create_reader()
    }

    #[inline]
    unsafe fn create_reader_tag_from_reader(&self, _parent: &Self::ReaderTag) -> Self::ReaderTag {
        self.create_reader()
    }

    #[inline]
    fn dangling_reader_tag() -> Self::ReaderTag {
        ReaderTag(())
    }

    #[inline]
    fn validate_swap(
        &self,
        _: &mut Self::WriterTag,
    ) -> Result<Self::ValidationToken, Self::ValidationError> {
        Ok(ValidationToken(()))
    }

    #[inline]
    unsafe fn capture_readers(
        &self,
        _: &mut Self::WriterTag,
        _: Self::ValidationToken,
    ) -> Self::Capture {
        let generation = self.generation.get();
        self.generation.set(generation.wrapping_add(2));
        let head = self.ptr.get();

        if head.is_null() {
            return Capture {
                generation: 0,
                start: ptr::null_mut(),
            };
        }

        let mut ptr = head;
        let mut start = ptr::null_mut::<ActiveReader>();
        let mut prev = head;

        // SAFETY: we never remove links from the linked list so the ptr is either null or valid
        while let Some(active_reader) = unsafe { ptr.as_ref() } {
            let current = active_reader.generation.get();

            if current == generation {
                // set the first node that has a generation, then set start
                if start.is_null() {
                    start = ptr;
                } else {
                    // SAFETY: the `next_captured` field is only modified by the writer while we have either:
                    // * exclusive access to the writer tag or
                    // * exclusive access to the capture and shared access to the writer tag .
                    //
                    // Since we have exclusive access to the writer tag right now, we can't race with `have_readers_exited`
                    // because that has shared access to the writer tag.
                    unsafe { (*prev).next_captured = ptr }
                }

                prev = ptr;
            }

            ptr = active_reader.next;
        }

        // SAFETY: the `next_captured` field is only modified by the writer while we have either:
        // * exclusive access to the writer tag or
        // * exclusive access to the capture and shared access to the writer tag .
        //
        // Since we have exclusive access to the writer tag right now, we can't race with `have_readers_exited`
        // because that has shared access to the writer tag.
        unsafe { (*prev).next_captured = ptr::null_mut() }

        Capture {
            generation,
            start: head,
        }
    }

    #[inline]
    unsafe fn have_readers_exited(&self, _: &Self::WriterTag, capture: &mut Self::Capture) -> bool {
        // SAFETY: this ptr is guarnteed to be a sublist of `self.ptr.load(_)`
        // because we got it in `capture_readers`
        let mut ptr = capture.start;
        let generation = capture.generation;
        let mut prev = &mut capture.start;

        let mut have_readers_exited = true;

        while !ptr.is_null() {
            // SAFETY: we never remove links from the linked list so the ptr is either null or valid
            // end is a node later in the list or null so all nodes between are valid
            let active_reader = unsafe { &*ptr };
            let current = active_reader.generation.get();

            let next = active_reader.next_captured;

            let reader_generation = current;

            debug_assert!(
                reader_generation == 0
                    || reader_generation == generation
                    || reader_generation == generation.wrapping_add(1),
                "invalid generation pair {generation} / {reader_generation}"
            );

            if reader_generation != generation {
                *prev = next;
            } else {
                have_readers_exited = false;
                // SAFETY: the `next_captured` field is only modified by the writer while we have either:
                // * exclusive access to the writer tag or
                // * exclusive access to the capture and shared access to the writer tag .
                //
                // Since we have shared access to the writer tag right now, we can't race with `capture_readers`
                // because that has exclusive access to the writer tag.
                // Since we have exclusvie access to the capture and there can't be more than one in-progress swap,
                // at a time, we can't race with `have_readers_exited`. So it's fine to get an exclusive reference
                // to `next_captured`.
                prev = unsafe { &mut (*ptr).next_captured };
            }

            ptr = next;
        }

        have_readers_exited
    }

    #[inline]
    unsafe fn begin_read_guard(&self, _: &mut Self::ReaderTag) -> Self::ReaderGuard {
        let head = self.ptr.get();
        let mut ptr = head;
        let generation = self.generation.get();

        // SAFETY: we never remove links from the linked list so the ptr is either null or valid
        while let Some(active_reader) = unsafe { ptr.as_ref() } {
            if active_reader.generation.get() == 0 {
                active_reader.generation.set(generation);
                return ReaderGuard(ptr);
            }

            ptr = active_reader.next;
        }

        self.begin_read_guard_slow(head, generation)
    }

    #[inline]
    unsafe fn end_read_guard(&self, _: &mut Self::ReaderTag, guard: Self::ReaderGuard) {
        // SAFETY: we never remove links from the linked list
        // and we only create valid links for `ReaderGuard`
        // so the link in the guard is still valid
        unsafe { (*guard.0).generation.set(0) };
    }

    #[cold]
    fn pause(&self, _writer: &Self::WriterTag, _pause: &mut Self::Pause) {
        panic!(
            "cannot swap buffers using local hazard strategy while there are readers in the buffer"
        )
    }
}

impl LocalHazardStrategy {
    /// The slow path of begin_read_guard which neeeds to allocate
    /// this should only happen if there are many readers aquiring
    /// for a read guard at the same time
    #[cold]
    fn begin_read_guard_slow(&self, head: *mut ActiveReader, generation: u32) -> ReaderGuard {
        let ptr = Box::into_raw(Box::new(ActiveReader {
            next: head,
            next_captured: ptr::null_mut(),
            generation: Cell::new(generation),
        }));

        self.ptr.set(ptr);

        ReaderGuard(ptr)
    }
}

impl Drop for LocalHazardStrategy {
    fn drop(&mut self) {
        let mut ptr = *self.ptr.get_mut();

        while !ptr.is_null() {
            // SAFETY: we never remove links from the linked list so the ptr is either null or valid
            // and we checked that the current link is non-null
            let next = unsafe { (*ptr).next };

            // SAFETY: we never remove links from the linked list so the ptr is either null or valid
            // and we checked that the current link is non-null
            unsafe { Box::from_raw(ptr) };

            ptr = next;
        }
    }
}

#[cfg(test)]
mod test {

    #[test]
    fn test_local_tracking() {
        let mut shared = crate::raw::Shared::from_raw_parts(
            super::LocalHazardStrategy::new(),
            crate::raw::SizedRawDoubleBuffer::new(0, 0),
        );
        let mut writer = crate::raw::Writer::new(&mut shared);

        let mut reader = writer.reader();

        let split_mut = writer.split_mut();
        *split_mut.writer = 10;
        let mut reader2 = reader;
        let a = reader.get();

        let mut writer = crate::delayed::DelayedWriter::from(writer);

        writer.start_buffer_swap();

        let b = reader2.get();

        assert!(!writer.is_swap_finished());

        drop(a);

        assert!(writer.is_swap_finished());

        drop(b);

        // assert_eq!(*reader.get(), 10);
        // let split_mut = writer.split_mut();
        // *split_mut.writer = 20;
        // assert_eq!(*reader.get(), 10);

        // writer.try_swap_buffers().unwrap();

        // assert_eq!(*reader.get(), 20);

        // let mut reader2 = reader.clone();
        // let _a = reader.get();

        // // SAFETY: we don't call any &mut self methods on writer any more
        // let mut swap = unsafe { writer.try_start_buffer_swap() }.unwrap();

        // assert!(!writer.is_swap_finished(&mut swap));

        // drop(_a);
        // let _a = reader2.get();

        // assert!(writer.is_swap_finished(&mut swap));
    }
}
