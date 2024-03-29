//! an sync strategy which precisely which readers are actually reading from the buffer

use core::sync::atomic::{AtomicUsize, Ordering};
use std::{sync::Arc, thread_local, time::Duration, vec::Vec};

#[cfg(feature = "parking_lot")]
use parking_lot::{Condvar, Mutex};
#[cfg(not(feature = "parking_lot"))]
use std::sync::{Condvar, Mutex, PoisonError};

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
        #[allow(unused_mut)]
        let mut readers = self.readers.lock();
        #[cfg(not(feature = "parking_lot"))]
        let mut readers = readers.unwrap_or_else(PoisonError::into_inner);
        readers.push(tag.generation.clone());
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
    type Pause = usize;

    #[inline]
    unsafe fn create_writer_tag(&mut self) -> Self::WriterTag {
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

        #[allow(unused_mut)]
        let mut readers = self.readers.lock();
        #[cfg(not(feature = "parking_lot"))]
        let mut readers = readers.unwrap_or_else(PoisonError::into_inner);

        readers.retain(|tag| {
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

    unsafe fn have_readers_exited(
        &self,
        _writer: &Self::WriterTag,
        capture: &mut Self::Capture,
    ) -> bool {
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

    fn pause(&self, _writer: &Self::WriterTag, pause: &mut usize) {
        /// the max number of growth iterations
        const MAX_ITERATIONS: usize = 20;
        /// the maximum timeout
        const MAX_TIMEOUT: Duration = Duration::from_secs(1);

        let pause_time = *pause;
        *pause += 1;
        *pause = (*pause).min(MAX_ITERATIONS);

        #[allow(unused_mut)]
        let mut readers = self.readers.lock();
        #[cfg(not(feature = "parking_lot"))]
        let readers = readers.unwrap_or_else(PoisonError::into_inner);

        let timeout = MAX_TIMEOUT * (1 << pause_time) / (1 << MAX_ITERATIONS);

        #[allow(clippy::let_underscore_lock)]
        #[cfg(not(feature = "parking_lot"))]
        let _ = self.cv.wait_timeout(readers, timeout);

        #[cfg(feature = "parking_lot")]
        let _ = self.cv.wait_for(&mut readers, timeout);
    }
}

impl<B: crate::interface::RawBuffers> crate::interface::DefaultOwned<B> for TrackingStrategy {
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
    type SizedPtr<'a> = &'a crate::raw::Shared<TrackingStrategy, crate::raw::RawDBuf<()>>;
    type SlicePtr<'a> = &'a crate::raw::Shared<TrackingStrategy, crate::raw::SliceRawDbuf<[()]>>;

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
#[cfg_attr(feature = "loom", ignore = "when using loom: ignore normal tests")]
fn test_local_tracking() {
    let mut shared =
        crate::raw::Shared::from_raw_parts(TrackingStrategy::new(), crate::raw::RawDBuf::new(0, 0));
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

    // SAFETY: we created the swap above
    assert!(!unsafe { writer.is_swap_finished(&mut swap) });

    drop(_a);
    let _a = reader2.get();

    // SAFETY: we created the swap above
    assert!(unsafe { writer.is_swap_finished(&mut swap) });
}
