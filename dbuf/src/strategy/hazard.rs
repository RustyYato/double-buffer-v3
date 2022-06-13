#![allow(clippy::missing_docs_in_private_items)]

use core::{
    num::NonZeroU32,
    ptr,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};
use std::sync::atomic::AtomicU64;

use crate::interface::Strategy;

/// A hazard pointer strategy
///
/// a lock-free synchronization strategy
///
/// see module level docs for details
pub struct HazardStrategy {
    /// the head of the append-only linked list of possibly active readers
    ptr: AtomicPtr<ActiveReader>,
    /// the current reader index
    reader_index: AtomicU32,
    /// the current generation
    generation: AtomicU32,
}

/// a link in the linked list of possibly active readers
struct ActiveReader {
    /// the next link in the list
    ///
    /// if null => no next link
    /// if non-null => the next link
    next: *mut ActiveReader,

    /// the next link in the captured list
    ///
    /// this field is owned by the writer, readers may not touch it
    ///
    /// if null => no next link
    /// if non-null => the next link
    next_captured: *mut ActiveReader,

    /// both the reader index and generation of the reader
    /// where the layout is:
    /// [generation: u32, reader_index: u32]
    ///
    /// so we can use a single atomic instruction to access both
    current: AtomicU64,
}

impl HazardStrategy {
    /// Create a new hazard strategy
    pub const fn new() -> Self {
        HazardStrategy {
            ptr: AtomicPtr::new(ptr::null_mut()),
            reader_index: AtomicU32::new(0),
            generation: AtomicU32::new(1),
        }
    }

    /// create a new reader tag
    fn create_reader(&self) -> ReaderTag {
        let id = NonZeroU32::new(1 + self.reader_index.fetch_add(1, Ordering::Relaxed))
            .expect("overlowed the number of readers");
        ReaderTag { id }
    }
}

/// the writer tag for [`TrackingStrategy`]
pub struct WriterTag(());
/// the reader tag for [`TrackingStrategy`]
pub struct ReaderTag {
    /// the index of this reader tag
    id: NonZeroU32,
}
/// the validation token for [`TrackingStrategy`]
pub struct ValidationToken {
    /// the generation that we captured
    generation: u32,
}
/// the capture token for [`TrackingStrategy`]
pub struct Capture {
    /// the captured generation
    generation: u32,
    /// the latest active reader for that generation
    start: *mut ActiveReader,
}
/// the reader guard for [`TrackingStrategy`]
pub struct ReaderGuard(*mut ActiveReader);

// SAFETY: Capture follows the normal rules for data access
// so we can implement Send and Sync for it
unsafe impl Send for Capture {}
// SAFETY: Capture follows the normal rules for data access
// so we can implement Send and Sync for it
unsafe impl Sync for Capture {}

// SAFETY: ReaderGuard follows the normal rules for data access
// so we can implement Send and Sync for it
unsafe impl Send for ReaderGuard {}

// SAFETY: ReaderGuard follows the normal rules for data access
// so we can implement Send and Sync for it
unsafe impl Sync for ReaderGuard {}

// SAFETY: FIXME
unsafe impl Strategy for HazardStrategy {
    type WriterTag = WriterTag;
    type ReaderTag = ReaderTag;
    type Which = crate::raw::AtomicFlag;
    type ValidationToken = ValidationToken;
    type ValidationError = core::convert::Infallible;
    type Capture = Capture;
    type ReaderGuard = ReaderGuard;
    type Pause = ();

    unsafe fn create_writer_tag(&mut self) -> Self::WriterTag {
        WriterTag(())
    }

    unsafe fn create_reader_tag_from_writer(&self, _parent: &Self::WriterTag) -> Self::ReaderTag {
        self.create_reader()
    }

    unsafe fn create_reader_tag_from_reader(&self, _parent: &Self::ReaderTag) -> Self::ReaderTag {
        self.create_reader()
    }

    fn dangling_reader_tag() -> Self::ReaderTag {
        ReaderTag {
            id: NonZeroU32::new(u32::MAX).unwrap(),
        }
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
        let head = self.ptr.load(Ordering::Acquire);
        let mut ptr = head;
        let mut start = ptr::null_mut::<ActiveReader>();
        let mut prev = head;

        // SAFETY: we never remove links from the linked list so the ptr is either null or valid
        while let Some(active_reader) = unsafe { ptr.as_ref() } {
            let current = active_reader.current.load(Ordering::Acquire);

            if (current >> 32) as u32 == generation {
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
            let current = active_reader.current.load(Ordering::Acquire);

            let next = active_reader.next_captured;

            let reader_generation = (current >> 32) as u32;

            debug_assert!(
                reader_generation == generation || reader_generation == generation.wrapping_add(1)
            );

            if reader_generation == generation {
                *prev = next;
                have_readers_exited = false;
            } else {
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

    unsafe fn begin_read_guard(&self, reader: &mut Self::ReaderTag) -> Self::ReaderGuard {
        /// see panic message
        fn read_failed() -> ! {
            panic!("Tried to re-insert this reader into the hazard pointer list, this should only be possible if the reader guard was leaked")
        }

        let mut ptr = self.ptr.load(Ordering::Relaxed);

        let generation = self.generation.load(Ordering::Acquire);

        let id = u64::from(reader.id.get()) | (u64::from(generation) << 32);

        // SAFETY: we never remove links from the linked list so the ptr is either null or valid
        while let Some(active_reader) = unsafe { ptr.as_ref() } {
            match active_reader.current.compare_exchange(
                0,
                id,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => return ReaderGuard(ptr),
                Err(e) => {
                    if e as u32 == reader.id.get() {
                        read_failed();
                    }
                }
            }

            ptr = active_reader.next;
        }

        let active_reader = Box::into_raw(Box::new(ActiveReader {
            next: ptr::null_mut(),
            next_captured: ptr::null_mut(),
            current: AtomicU64::new(id),
        }));

        let mut ptr = self.ptr.load(Ordering::Acquire);

        loop {
            // SAFETY: we never remove links from the linked list so the ptr is either null or valid
            unsafe { (*active_reader).next = ptr }

            if let Err(curr) = self.ptr.compare_exchange_weak(
                ptr,
                active_reader,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                ptr = curr
            } else {
                break ReaderGuard(active_reader);
            }
        }
    }

    unsafe fn end_read_guard(&self, _: &mut Self::ReaderTag, guard: Self::ReaderGuard) {
        // SAFETY: we never remove links from the linked list
        // and we only create valid links for `ReaderGuard`
        // so the link in the guard is still valid
        unsafe { (*guard.0).current.store(0, Ordering::Release) }
    }
}

impl Drop for HazardStrategy {
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
        let mut shared = crate::raw::Shared::new(
            super::HazardStrategy::new(),
            crate::raw::SizedRawDoubleBuffer::new(0, 0),
        );
        let mut writer = crate::raw::Writer::new(&mut shared);

        let mut reader = writer.reader();

        let split_mut = writer.split_mut();
        *split_mut.writer = 10;
        let mut reader2 = reader.clone();
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
