//! an local strategy which precisely which readers are actually reading from the buffer

use core::cell::Cell;
use std::vec::Vec;

use crate::interface::Strategy;

/// the index type used to identify readers
type Index = usize;

/// An optimized local strategy which only counts how many active readers there are
pub struct LocalTrackingStrategy {
    /// the number of active readers
    active_readers: Cell<slab::Slab<Index>>,
    /// the next index type
    index: Cell<Index>,
}

impl LocalTrackingStrategy {
    /// Create a new local strategy
    pub fn new() -> Self {
        Self {
            active_readers: Cell::new(slab::Slab::new()),
            index: Cell::new(0),
        }
    }
}

impl Default for LocalTrackingStrategy {
    fn default() -> Self {
        Self::new()
    }
}

/// the writer tag for [`LocalTrackingStrategy`]
pub struct WriterTag(());
/// the reader tag for [`LocalTrackingStrategy`]
pub struct ReaderTag {
    /// the index of this reader tag
    index: Index,
    /// the guard index for the current
    guard_index: usize,
}
/// the validation token for [`LocalTrackingStrategy`]
pub struct ValidationToken(());
/// the capture token for [`LocalTrackingStrategy`]
pub struct Capture(Vec<(usize, usize)>);
/// the reader guard for [`LocalTrackingStrategy`]
pub struct ReaderGuard(());

impl LocalTrackingStrategy {
    /// create a new reader tag
    fn create_reader_tag(&self) -> ReaderTag {
        let index = self.index.get();
        self.index.set(
            self.index
                .get()
                .wrapping_add(2)
                .checked_sub(1)
                .expect("cannot overflow index"),
        );
        ReaderTag {
            index,
            guard_index: usize::MAX,
        }
    }
}

// SAFETY: FIXME
unsafe impl Strategy for LocalTrackingStrategy {
    type WriterTag = WriterTag;
    type ReaderTag = ReaderTag;
    type Which = crate::raw::Flag;
    type ValidationToken = ValidationToken;
    type ValidationError = core::convert::Infallible;
    type Capture = Capture;
    type ReaderGuard = ReaderGuard;
    type Pause = ();

    #[inline]
    unsafe fn create_writer_tag(&self) -> Self::WriterTag {
        WriterTag(())
    }

    #[inline]
    unsafe fn create_reader_tag_from_writer(&self, _parent: &Self::WriterTag) -> Self::ReaderTag {
        self.create_reader_tag()
    }

    #[inline]
    unsafe fn create_reader_tag_from_reader(&self, _parent: &Self::ReaderTag) -> Self::ReaderTag {
        self.create_reader_tag()
    }

    #[inline]
    fn dangling_reader_tag() -> Self::ReaderTag {
        ReaderTag {
            index: usize::MAX,
            guard_index: usize::MAX,
        }
    }

    #[inline]
    fn validate_swap(
        &self,
        _writer: &mut Self::WriterTag,
    ) -> Result<Self::ValidationToken, Self::ValidationError> {
        Ok(ValidationToken(()))
    }

    unsafe fn capture_readers(
        &self,
        _: &mut Self::WriterTag,
        _: Self::ValidationToken,
    ) -> Self::Capture {
        // SAFETY: capture_readers isn't reentrant or Sync so there can't be more than one `&mut` to active_readers
        let active_readers = unsafe { &mut *self.active_readers.as_ptr() };

        let mut capture = Vec::new();

        capture.reserve(active_readers.len());
        for (guard_index, &index) in active_readers.iter() {
            capture.push((guard_index, index));
        }

        Capture(capture)
    }

    fn have_readers_exited(&self, _writer: &Self::WriterTag, capture: &mut Self::Capture) -> bool {
        // SAFETY: have_readers_exited isn't reentrant or Sync so there can't be more than one `&mut` to active_readers
        let active_readers = unsafe { &mut *self.active_readers.as_ptr() };

        capture
            .0
            .retain(|&(guard_index, index)| active_readers.get(guard_index) == Some(&index));

        capture.0.is_empty()
    }

    #[inline]
    unsafe fn begin_read_guard(&self, reader: &mut Self::ReaderTag) -> Self::ReaderGuard {
        assert!(
            reader.guard_index == usize::MAX,
            "detected a leaked read guard"
        );
        assert_ne!(reader.index, usize::MAX);
        // SAFETY: begin_read_guard isn't reentrant or Sync so there can't be more than one `&mut` to active_readers
        let active_readers = unsafe { &mut *self.active_readers.as_ptr() };
        reader.guard_index = active_readers.insert(reader.index);
        ReaderGuard(())
    }

    #[inline]
    unsafe fn end_read_guard(&self, reader: &mut Self::ReaderTag, _guard: Self::ReaderGuard) {
        // SAFETY: end_read_guard isn't reentrant or Sync so there can't be more than one `&mut` to active_readers
        let active_readers = unsafe { &mut *self.active_readers.as_ptr() };
        let index = active_readers.remove(reader.guard_index);
        assert_eq!(index, reader.index);
        reader.guard_index = usize::MAX;
    }
}

#[test]
fn test_local_tracking() {
    let mut shared = crate::raw::Shared::new(
        LocalTrackingStrategy::new(),
        crate::raw::SizedRawDoubleBuffer::new(0, 0),
    );
    let mut writer = crate::raw::Writer::new(&mut shared);

    let mut reader = writer.reader();

    let split_mut = writer.split_mut();
    *split_mut.writer = 10;
    assert_eq!(*reader.get(), 0);

    writer.try_swap_buffers().unwrap();

    assert_eq!(*reader.get(), 10);
    let split_mut = writer.split_mut();
    *split_mut.writer = 20;
    assert_eq!(*reader.get(), 10);

    writer.try_swap_buffers().unwrap();

    assert_eq!(*reader.get(), 20);

    let mut reader2 = reader.clone();
    let _a = reader.get();

    // SAFETY: we don't call any &mut self methods on writer any more
    let mut swap = unsafe { writer.try_start_buffer_swap() }.unwrap();

    assert!(!writer.is_swap_finished(&mut swap));

    drop(_a);
    let _a = reader2.get();

    assert!(writer.is_swap_finished(&mut swap));
}
