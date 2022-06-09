//! an local strategy which precisely which readers are actually reading from the buffer

use core::cell::Cell;

use crate::interface::Strategy;

/// the index type used to identify readers
#[cfg(not(debug_assertions))]
type Index = ();
/// the index type used to identify readers
#[cfg(debug_assertions)]
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
            #[cfg(not(debug_assertions))]
            index: Cell::new(()),
            #[cfg(debug_assertions)]
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
#[derive(Clone, Copy)]
pub struct ReaderTag {
    /// the index of this reader tag
    index: Index,
    /// the guard index for the current
    guard_index: usize,
}
/// the validation token for [`LocalTrackingStrategy`]
pub struct ValidationToken(());
/// the capture token for [`LocalTrackingStrategy`]
pub struct Capture(());
/// the reader guard for [`LocalTrackingStrategy`]
pub struct ReaderGuard(());

impl LocalTrackingStrategy {
    /// create a new reader tag
    fn create_reader_tag(&self) -> ReaderTag {
        let index = self.index.get();
        #[cfg(debug_assertions)]
        self.index.set(
            self.index
                .get()
                .wrapping_sub(1)
                .checked_add(2)
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

    unsafe fn create_writer_tag(&self) -> Self::WriterTag {
        WriterTag(())
    }

    unsafe fn create_reader_tag_from_writer(&self, _parent: &Self::WriterTag) -> Self::ReaderTag {
        self.create_reader_tag()
    }

    unsafe fn create_reader_tag_from_reader(&self, _parent: &Self::ReaderTag) -> Self::ReaderTag {
        self.create_reader_tag()
    }

    fn dangling_reader_tag() -> Self::ReaderTag {
        ReaderTag {
            #[cfg(not(debug_assertions))]
            index: (),
            #[cfg(debug_assertions)]
            index: usize::MAX,
            guard_index: usize::MAX,
        }
    }

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
        Capture(())
    }

    fn have_readers_exited(&self, _writer: &Self::WriterTag, _capture: &mut Self::Capture) -> bool {
        true
    }

    unsafe fn begin_read_guard(&self, reader: &mut Self::ReaderTag) -> Self::ReaderGuard {
        assert_ne!(reader.guard_index, usize::MAX);
        // SAFETY: begin_read_guard isn't reentrant or Sync so there can't be more than one `&mut` to active_readers
        let active_readers = unsafe { &mut *self.active_readers.as_ptr() };
        active_readers.insert(reader.index);
        ReaderGuard(())
    }

    unsafe fn end_read_guard(&self, reader: &mut Self::ReaderTag, _guard: Self::ReaderGuard) {
        // SAFETY: begin_read_guard isn't reentrant or Sync so there can't be more than one `&mut` to active_readers
        let active_readers = unsafe { &mut *self.active_readers.as_ptr() };
        let index = active_readers.remove(reader.guard_index);
        assert_eq!(index, reader.index);
        reader.guard_index = usize::MAX;
    }
}
