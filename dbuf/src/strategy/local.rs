//! an optimized local strategy with minimal overhead

use core::cell::Cell;

use crate::interface::Strategy;

/// An optimized local strategy which only counts how many active readers there are
pub struct LocalStrategy {
    /// the number of active readers
    active_readers: Cell<usize>,
}

impl LocalStrategy {
    /// Create a new local strategy
    pub const fn new() -> Self {
        Self {
            active_readers: Cell::new(0),
        }
    }
}

impl Default for LocalStrategy {
    fn default() -> Self {
        Self::new()
    }
}

/// the writer tag for [`LocalStrategy`]
pub struct WriterTag(());
/// the reader tag for [`LocalStrategy`]
#[derive(Clone, Copy)]
pub struct ReaderTag(());
/// the validation token for [`LocalStrategy`]
pub struct ValidationToken(());
/// the validation error for [`LocalStrategy`]
pub struct ValidationError(());
/// the capture token for [`LocalStrategy`]
pub struct Capture(());
/// the reader guard for [`LocalStrategy`]
pub struct ReaderGuard(());

impl core::fmt::Debug for ValidationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Tried to swap buffers while there are active readers")
    }
}

// SAFETY: FIXME
unsafe impl Strategy for LocalStrategy {
    type WriterTag = WriterTag;
    type ReaderTag = ReaderTag;
    type Which = crate::raw::Flag;
    type ValidationToken = ValidationToken;
    type ValidationError = ValidationError;
    type Capture = Capture;
    type ReaderGuard = ReaderGuard;

    unsafe fn create_writer_tag(&self) -> Self::WriterTag {
        WriterTag(())
    }

    unsafe fn create_reader_tag_from_writer(&self, _parent: &Self::WriterTag) -> Self::ReaderTag {
        ReaderTag(())
    }

    unsafe fn create_reader_tag_from_reader(&self, _parent: &Self::ReaderTag) -> Self::ReaderTag {
        ReaderTag(())
    }

    fn dangling_reader_tag() -> Self::ReaderTag {
        ReaderTag(())
    }

    fn validate_swap(
        &self,
        _writer: &mut Self::WriterTag,
    ) -> Result<Self::ValidationToken, Self::ValidationError> {
        if self.active_readers.get() == 0 {
            Ok(ValidationToken(()))
        } else {
            Err(ValidationError(()))
        }
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

    unsafe fn begin_read_guard(&self, _reader: &mut Self::ReaderTag) -> Self::ReaderGuard {
        let count = self.active_readers.get();
        self.active_readers.set(
            count
                .checked_add(1)
                .expect("tried to create too many active readers"),
        );
        ReaderGuard(())
    }

    unsafe fn end_read_guard(&self, _reader: &mut Self::ReaderTag, _guard: Self::ReaderGuard) {
        let count = self.active_readers.get();
        self.active_readers.set(count - 1);
    }
}
