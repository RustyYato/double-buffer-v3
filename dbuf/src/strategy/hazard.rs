//! A hazard pointer strategy
//!
//! ## Basic overview:
//!
//! The [`HazardStrategy`] keeps track of the current generation, which
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
//! # use core::sync::atomic::AtomicU32;
//! struct ActiveReader {
//!     next: *mut ActiveReader,
//!     next_captured: *mut ActiveReader,
//!     generation: AtomicU32,
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
//! When a reader tries to acquire a read guard, it will follow the following steps until
//! it finds an available node
//!
//! * check it's local cache for an available node
//! * check the entire linked list for an available node
//! * create a new node and push it onto the list
//!
//! once it find sa node it will update it's local cache. Then when the read ends, it will
//! clear out the active reader in it's cache (but keep it in the cache).
//!
//! ### Swaps
//!
//! When the writer wants to swap
//! * the [`HazardStrategy`] will increment the generation counter (by 2 to stay odd)
//! * the writer will swap the buffers
//! * the [`HazardStrategy`] iterate over the entire list and setup the `next_captured` sub-sequence of
//! readers which are still in the previous generation.
//! * while this subsequence is non-empty the [`HazardStrategy`] will iterate over the sub-sequence and remove
//! elements from the sub-sequence which have are `EMPTY` or not in the same generation.

use core::ptr;
#[cfg(not(feature = "loom"))]
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
#[cfg(feature = "loom")]
use loom::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::boxed::Box;

use crate::{
    interface::{Strategy, WaitStrategy},
    wait::DefaultWait,
};

/// A hazard pointer strategy
///
/// a lock-free synchronization strategy
///
/// see module level docs for details
pub struct HazardStrategy<W = DefaultWait> {
    /// the head of the append-only linked list of possibly active readers
    ptr: AtomicPtr<ActiveReader>,
    /// the current generation
    generation: AtomicU32,
    /// the waiting strategy
    wait: W,
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
    generation: AtomicU32,
}

impl HazardStrategy {
    /// Create a new hazard strategy
    pub const fn new() -> Self {
        Self::with_wait_strategy(crate::wait::DefaultWait::new())
    }
}

impl<W: Default> Default for HazardStrategy<W> {
    fn default() -> Self {
        Self::with_wait_strategy(W::default())
    }
}

impl<W> HazardStrategy<W> {
    /// Create a new [`HazardStrategy`] with the given [`WaitStrategy`]
    #[cfg(not(feature = "loom"))]
    pub const fn with_wait_strategy(park: W) -> Self {
        Self {
            ptr: AtomicPtr::new(ptr::null_mut()),
            generation: AtomicU32::new(1),
            wait: park,
        }
    }

    /// Create a new [`HazardStrategy`] with the given [`WaitStrategy`]
    #[cfg(feature = "loom")]
    pub fn with_park_strategy(park: W) -> Self {
        Self {
            ptr: AtomicPtr::new(ptr::null_mut()),
            generation: AtomicU32::new(1),
            wait: park,
        }
    }

    /// create a new reader tag
    fn create_reader() -> ReaderTag {
        ReaderTag {
            node: ptr::null_mut(),
        }
    }
}

/// the writer tag for [`HazardStrategy`]
pub struct WriterTag(());
/// the reader tag for [`HazardStrategy`]
#[derive(Clone, Copy)]
pub struct ReaderTag {
    /// the node which the reader last used as active reader
    node: *mut ActiveReader,
}
/// the validation token for [`HazardStrategy`]
pub struct ValidationToken {
    /// the generation that we captured
    generation: u32,
}
/// the capture token for [`HazardStrategy`]
pub struct Capture {
    /// the captured generation
    generation: u32,
    /// the latest active reader for that generation
    start: *mut ActiveReader,
}
/// the reader guard for [`HazardStrategy`]
pub struct ReaderGuard(());

// SAFETY: ReaderTag follows the normal rules for data access
// so we can implement Send and Sync for it
unsafe impl Send for ReaderTag {}
// SAFETY: ReaderTag follows the normal rules for data access
// so we can implement Send and Sync for it
unsafe impl Sync for ReaderTag {}

// SAFETY: Capture follows the normal rules for data access
// so we can implement Send and Sync for it
unsafe impl Send for Capture {}
// SAFETY: Capture follows the normal rules for data access
// so we can implement Send and Sync for it
unsafe impl Sync for Capture {}

// SAFETY: FIXME
unsafe impl<W: WaitStrategy> Strategy for HazardStrategy<W> {
    type WriterTag = WriterTag;
    type ReaderTag = ReaderTag;
    type Which = crate::raw::AtomicFlag;
    type ValidationToken = ValidationToken;
    type ValidationError = core::convert::Infallible;
    type Capture = Capture;
    type ReaderGuard = ReaderGuard;
    type Pause = W::State;

    const READER_TAG_NEEDS_CONSTRUCTION: bool = false;

    unsafe fn create_writer_tag(&mut self) -> Self::WriterTag {
        WriterTag(())
    }

    unsafe fn create_reader_tag_from_writer(&self, _parent: &Self::WriterTag) -> Self::ReaderTag {
        Self::create_reader()
    }

    unsafe fn create_reader_tag_from_reader(&self, _parent: &Self::ReaderTag) -> Self::ReaderTag {
        Self::create_reader()
    }

    fn dangling_reader_tag() -> Self::ReaderTag {
        Self::create_reader()
    }

    fn validate_swap(
        &self,
        _: &mut Self::WriterTag,
    ) -> Result<Self::ValidationToken, Self::ValidationError> {
        let generation = self.generation.fetch_add(2, Ordering::AcqRel);

        Ok(ValidationToken { generation })
    }

    unsafe fn capture_readers(
        &self,
        _: &mut Self::WriterTag,
        ValidationToken { generation }: Self::ValidationToken,
    ) -> Self::Capture {
        // create a sub-sequence of nodes which are in the given generation
        let head = self.ptr.load(Ordering::Acquire);

        if head.is_null() {
            return Capture {
                generation: 0,
                start: ptr::null_mut(),
            };
        }

        let mut ptr = head;
        let mut sub_sequence_start = ptr::null_mut::<ActiveReader>();

        // this is not null in case the sub-sequence is empty
        // we can avoid a branch on the final set to `next_captured`
        // by having it do a useless write to the head of the list
        let mut sub_sequence_prev = ptr;

        // SAFETY: we never remove links from the linked list so the ptr is either null or valid
        while let Some(active_reader) = unsafe { ptr.as_ref() } {
            let current = active_reader.generation.load(Ordering::Acquire);

            if current == generation {
                if sub_sequence_start.is_null() {
                    // if this is the first node, then set it to the start
                    sub_sequence_start = ptr;
                } else {
                    // otherwise set the previous node's `next_captured` field to continue the sub-sequence

                    // SAFETY: the `next_captured` field is only modified by the writer while we have either:
                    // * exclusive access to the writer tag or
                    // * exclusive access to the capture and shared access to the writer tag .
                    //
                    // Since we have exclusive access to the writer tag right now, we can't race with `have_readers_exited`
                    // because that has shared access to the writer tag.
                    unsafe { (*sub_sequence_prev).next_captured = ptr }
                }

                // update the previous node
                sub_sequence_prev = ptr;
            }

            // then continue on to the rest of the list
            ptr = active_reader.next;
        }

        // set the last node's `next_captured` to null to signify that it's the last in the sub-sequence

        // SAFETY:
        // * the ptr is valid because `head` and all nodes in the linked list are non-null and never deallocated until `Drop`
        //
        //  the `next_captured` field is only modified by the writer while we have either:
        // * exclusive access to the writer tag or
        // * exclusive access to the capture and shared access to the writer tag .
        //
        // Since we have exclusive access to the writer tag right now, we can't race with `have_readers_exited`
        // because that has shared access to the writer tag.
        unsafe { (*sub_sequence_prev).next_captured = ptr::null_mut() }

        Capture {
            generation,
            start: head,
        }
    }

    unsafe fn have_readers_exited(&self, _: &Self::WriterTag, capture: &mut Self::Capture) -> bool {
        // here we iterate over the capture sub-sequence and remove nodes which are no longer in the previous generation

        // SAFETY: this ptr is guarnteed to be a sublist of `self.ptr.load(_)`
        // because we got it in `capture_readers`
        let mut ptr = capture.start;
        let generation = capture.generation;

        // SAFETY: we never remove links from the linked list so the ptr is either null or valid
        // end is a node later in the list or null so all nodes between are valid
        while let Some(active_reader) = unsafe { ptr.as_ref() } {
            let current = active_reader.generation.load(Ordering::Acquire);
            let next = active_reader.next_captured;
            let reader_generation = current;

            debug_assert!(
                reader_generation == 0
                    || reader_generation == generation
                    || reader_generation == generation.wrapping_add(2),
                "invalid generation pair {generation} / {reader_generation}"
            );

            if reader_generation == generation {
                // if the reader is still in the buffer, then update the capture sub-sequence
                // to the current node (because all previous nodes are out of the sub-sequence,
                // if they were not, we would have exitted earlier)
                capture.start = ptr;

                return false;
            }

            ptr = next;
        }

        true
    }

    #[inline]
    unsafe fn begin_read_guard(&self, reader: &mut Self::ReaderTag) -> Self::ReaderGuard {
        let generation = self.generation.load(Ordering::Acquire);

        // SAFETY: the reader node is either null or valid and points
        // into the `self.ptr` linked list
        match unsafe { reader.node.as_ref() } {
            Some(active_reader) => {
                // first check the local cache to see if there's an available node
                // we use this cache to eliminate contention between nodes on different threads
                // but this allows different readers to use the same active reader node
                // as long as their read access patterns don't overlap
                //
                // with the cache, there will usually only be this reader and the writer
                // who access this node, so there is minimal contention.

                match active_reader.generation.compare_exchange_weak(
                    0,
                    generation,
                    Ordering::Release,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => ReaderGuard(()),
                    Err(_generation) => {
                        // if the cached node is in use by some other reader, then just allocate a new node
                        // this minimizes contention and should improve throughput at the expense of a little memory
                        let node = self.load_read_guard_slow(generation);
                        reader.node = node;

                        ReaderGuard(())
                    }
                }
            }
            None => {
                // if we don't have a cached node, look for an available node
                let node = self.load_read_guard(generation);
                reader.node = node;

                ReaderGuard(())
            }
        }
    }

    unsafe fn end_read_guard(&self, reader: &mut Self::ReaderTag, _: Self::ReaderGuard) {
        // SAFETY: we never remove links from the linked list
        // and we only create valid links for `ReaderGuard`
        // so the link in the guard is still valid
        unsafe { (*reader.node).generation.store(0, Ordering::Release) };

        self.wait.notify();
    }

    fn pause(&self, _writer: &Self::WriterTag, pause: &mut Self::Pause) {
        self.wait.wait(pause);
    }
}

impl<B: crate::interface::RawBuffers> crate::interface::DefaultOwned<B> for HazardStrategy {
    type IntoStrongRefWithWeak = crate::ptrs::alloc::OwnedWithWeak<Self, B>;
    type StrongRefWithWeak = crate::ptrs::alloc::OwnedStrong<Self, B>;
    type WeakRef = crate::ptrs::alloc::OwnedWeak<Self, B>;

    type IntoStrongRef = crate::ptrs::alloc::Owned<Self, B>;
    type StrongRef = crate::ptrs::alloc::OwnedPtr<Self, B>;

    fn build_with_weak(self, buffers: B) -> Self::IntoStrongRefWithWeak {
        crate::ptrs::alloc::OwnedWithWeak::new(crate::raw::Shared::from_raw_parts(self, buffers))
    }

    fn build(self, buffers: B) -> Self::IntoStrongRef {
        crate::ptrs::alloc::Owned::new(crate::raw::Shared::from_raw_parts(self, buffers))
    }
}

impl<W> HazardStrategy<W> {
    /// Load the reader guard from the linked list because the reader node cache failed
    #[cold]
    fn load_read_guard(&self, generation: u32) -> *mut ActiveReader {
        // load the entire linked list
        let mut ptr = self.ptr.load(Ordering::Acquire);

        // SAFETY: we never remove links from the linked list so the ptr is either null or valid
        while let Some(active_reader) = unsafe { ptr.as_ref() } {
            // check if the given active reader is empty, and use it if it is
            if active_reader
                .generation
                .compare_exchange_weak(0, generation, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return ptr;
            }

            // otherwise move on to the next active reader
            ptr = active_reader.next;
        }

        // if none of the active readers are empty (usually because of high contention or spurious failures of `compare_exchange_weak`)
        // then we should create a new node and push it onto the list
        self.load_read_guard_slow(generation)
    }

    /// The slow path of begin_read_guard which neeeds to allocate
    /// this should only happen if there are many readers aquiring
    /// for a read guard at the same time
    #[cold]
    fn load_read_guard_slow(&self, generation: u32) -> *mut ActiveReader {
        // the list is full so allocate a new node to push onto the head of the list
        let active_reader = Box::into_raw(Box::new(ActiveReader {
            next: ptr::null_mut(),
            next_captured: ptr::null_mut(),
            generation: AtomicU32::new(generation),
        }));

        let mut ptr = self.ptr.load(Ordering::Acquire);

        loop {
            // set the next ptr to the current head
            // SAFETY: we never remove links from the linked list so the ptr is either null or valid
            unsafe { (*active_reader).next = ptr }

            // and swap in new node with the head
            if let Err(curr) = self.ptr.compare_exchange_weak(
                ptr,
                active_reader,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                ptr = curr
            } else {
                break active_reader;
            }
        }
    }
}

impl<W> Drop for HazardStrategy<W> {
    fn drop(&mut self) {
        #[cfg(feature = "loom")]
        let mut ptr = self.ptr.with_mut(|a| *a);
        #[cfg(not(feature = "loom"))]
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
    #[cfg_attr(feature = "loom", ignore = "when using loom: ignore normal tests")]
    fn test_local_tracking() {
        let mut shared = crate::raw::Shared::from_raw_parts(
            super::HazardStrategy::new(),
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

    #[test]
    #[cfg(feature = "loom")]
    #[cfg(feature = "alloc")]
    fn test_mutlithreaded() {
        use crate::wait::SpinWait;

        loom::model(|| {
            let mut shared = crate::raw::Shared::new(
                super::HazardStrategy::<SpinWait>::default(),
                crate::raw::SizedRawDoubleBuffer::new(0, 0),
            );
            let mut writer = crate::raw::Writer::new(crate::ptrs::alloc::Owned::new(shared));

            let mut reader = writer.reader();

            loom::thread::spawn(move || {
                let a = reader.get();
                let _a = &*a;

                loom::thread::yield_now();
            });

            let mut reader = writer.reader();

            loom::thread::spawn(move || {
                let a = reader.get();
                let _a = &*a;

                loom::thread::yield_now();
            });

            loom::thread::spawn(move || {
                writer.swap_buffers();
                loom::thread::yield_now();
            });
        })

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
