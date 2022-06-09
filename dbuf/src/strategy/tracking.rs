//! an sync strategy which precisely which readers are actually reading from the buffer

use core::sync::atomic::{AtomicUsize, Ordering};
use std::{
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread_local,
    vec::Vec,
};

use crate::interface::Strategy;

/// A sync strategy which allows
pub struct TrackingStrategy {
    /// the number of active readers
    readers: Mutex<Vec<Arc<AtomicUsize>>>,
    /// a condvar to wait for readers
    cv: Condvar,
}

impl TrackingStrategy {
    /// Create a new local strategy
    pub fn new() -> Self {
        Self {
            readers: Mutex::new(Vec::new()),
            cv: Condvar::new(),
        }
    }
}

impl Default for TrackingStrategy {
    fn default() -> Self {
        Self::new()
    }
}

/// the writer tag for [`TrackingStrategy`]
pub struct WriterTag(());
/// the reader tag for [`TrackingStrategy`]
pub struct ReaderTag {
    /// the index of this reader tag
    generation: Arc<AtomicUsize>,
}
/// the validation token for [`TrackingStrategy`]
pub struct ValidationToken(());
/// the capture token for [`TrackingStrategy`]
pub struct Capture(Vec<(usize, Arc<AtomicUsize>)>);
/// the reader guard for [`TrackingStrategy`]
pub struct ReaderGuard(());

impl TrackingStrategy {
    /// create a new reader tag
    fn create_reader_tag(&self) -> ReaderTag {
        let tag = ReaderTag {
            generation: Arc::new(AtomicUsize::new(0)),
        };
        self.readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(tag.generation.clone());
        tag
    }
}

// SAFETY: FIXME
unsafe impl Strategy for TrackingStrategy {
    type WriterTag = WriterTag;
    type ReaderTag = ReaderTag;
    type Which = crate::raw::AtomicFlag;
    type ValidationToken = ValidationToken;
    type ValidationError = core::convert::Infallible;
    type Capture = Capture;
    type ReaderGuard = ReaderGuard;

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
        std::thread_local! {
            static DANGLING: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0))
        }
        ReaderTag {
            generation: DANGLING.with(Clone::clone),
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
        let mut capture = Vec::new();

        self.readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|tag| {
                if Arc::strong_count(tag) == 1 {
                    false
                } else {
                    let generation = tag.load(Ordering::Acquire);

                    if generation % 2 == 1 {
                        capture.push((generation, tag.clone()))
                    }

                    true
                }
            });

        Capture(capture)
    }

    fn have_readers_exited(&self, _writer: &Self::WriterTag, capture: &mut Self::Capture) -> bool {
        // SAFETY: have_readers_exited isn't reentrant or Sync so there can't be more than one `&mut` to active_readers
        capture
            .0
            .retain(|(generation, tag)| *generation == tag.load(Ordering::Relaxed));

        let is_empty = capture.0.is_empty();

        if is_empty {
            core::sync::atomic::fence(Ordering::Release);
        }

        is_empty
    }

    #[inline]
    unsafe fn begin_read_guard(&self, reader: &mut Self::ReaderTag) -> Self::ReaderGuard {
        reader.generation.fetch_add(1, Ordering::Release);
        ReaderGuard(())
    }

    #[inline]
    unsafe fn end_read_guard(&self, reader: &mut Self::ReaderTag, _guard: Self::ReaderGuard) {
        reader.generation.fetch_add(1, Ordering::Release);
        self.cv.notify_one();
    }

    fn pause(&self, _writer: &Self::WriterTag) {
        let guard = self.readers.lock().unwrap_or_else(PoisonError::into_inner);

        #[allow(clippy::let_underscore_lock)]
        let _ = self
            .cv
            .wait_timeout(guard, core::time::Duration::from_micros(100));
    }
}

#[allow(unused, clippy::missing_docs_in_private_items)]
fn assert_send<T: ?Sized + Send>() {}

#[allow(unused, clippy::missing_docs_in_private_items)]
fn assert_sync<T: ?Sized + Sync>() {}

#[allow(
    unused,
    path_statements,
    clippy::no_effect,
    clippy::missing_docs_in_private_items
)]
#[allow()]
fn _test_bounds() {
    type SizedPtr<'a> =
        &'a crate::raw::Shared<TrackingStrategy, crate::raw::SizedRawDoubleBuffer<()>>;
    type SlicePtr<'a> =
        &'a crate::raw::Shared<TrackingStrategy, crate::raw::SliceRawDoubleBuffer<[()]>>;

    assert_send::<TrackingStrategy>;
    assert_sync::<TrackingStrategy>;

    assert_send::<crate::raw::Writer<SizedPtr>>;
    assert_sync::<crate::raw::Writer<SizedPtr>>;

    assert_send::<crate::raw::Reader<SizedPtr>>;
    assert_sync::<crate::raw::Reader<SizedPtr>>;

    assert_send::<crate::raw::Writer<SlicePtr>>;
    assert_sync::<crate::raw::Writer<SlicePtr>>;

    assert_send::<crate::raw::Reader<SlicePtr>>;
    assert_sync::<crate::raw::Reader<SlicePtr>>;
}

#[test]
fn test_local_tracking() {
    let mut shared = crate::raw::Shared::new(
        TrackingStrategy::new(),
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
