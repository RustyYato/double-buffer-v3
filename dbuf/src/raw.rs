//! the raw building blocks of a double buffer

use core::{cell::UnsafeCell, ptr, sync::atomic::Ordering};

use crate::interface::{RawBuffers, Strategy, Which, WhichOf};

mod reader;
mod writer;

pub use reader::Reader;
pub use writer::Writer;

/// The shared state in required to manage a double buffer
pub struct Shared<S, B: ?Sized, W = WhichOf<S>> {
    /// the strategy used to syncronize the double buffer
    strategy: S,
    /// a boolean flag for which buffer is in front
    which: W,
    /// the buffers theselves
    buffers: B,
}

impl<S: Strategy, B: RawBuffers> Shared<S, B> {
    /// Create a new shared state to manage the double buffer
    pub fn new(strategy: S, buffers: B) -> Self {
        Self {
            strategy,
            which: Which::INIT,
            buffers,
        }
    }
}

/// a sized raw double buffer
///
/// it contains two instances of T which are the two buffers
#[repr(transparent)]
pub struct SizedRawDoubleBuffer<T>(UnsafeCell<[T; 2]>);

// SAFETY:
// * (T: Send) we allow getting a mutable refrence to T from a mutable reference to Self
unsafe impl<T: Send> Send for SizedRawDoubleBuffer<T> {}
// SAFETY:
// * (T: Send) we allow getting a mutable refrence to T from a shared reference to Self
// * (T: Sync) we allow getting a shared refrence to T from a shared reference to Self
unsafe impl<T: Send + Sync> Sync for SizedRawDoubleBuffer<T> {}

/// a slice raw double buffer
///
/// the
#[repr(transparent)]
pub struct SliceRawDoubleBuffer<T>(UnsafeCell<[T]>);

// SAFETY:
// * (T: Send) we allow getting a mutable refrence to T from a mutable reference to Self
unsafe impl<T: Send> Send for SliceRawDoubleBuffer<T> {}
// SAFETY:
// * (T: Send) we allow getting a mutable refrence to T from a shared reference to Self
// * (T: Sync) we allow getting a shared refrence to T from a shared reference to Self
unsafe impl<T: Send + Sync> Sync for SliceRawDoubleBuffer<T> {}

impl<T> SizedRawDoubleBuffer<T> {
    /// Create a new sized raw double buffer
    pub const fn new(front: T, back: T) -> Self {
        Self(UnsafeCell::new([front, back]))
    }
}

impl<T> SliceRawDoubleBuffer<T> {
    /// Create a new slice raw double buffer
    ///
    /// The length of the slice must be even
    pub fn new(slice: &mut [T]) -> &mut Self {
        assert!(slice.len() % 2 == 0);
        // Safety: Self has the same representation as [T]
        unsafe { &mut *(slice as *mut [T] as *mut Self) }
    }
}

// Safety:
// * the two pointers returned from get are always valid
// * they are disjoint
// * the data is not dereferenced
unsafe impl<T> RawBuffers for SizedRawDoubleBuffer<T> {
    type Buffer = T;

    fn get(&self, which: bool) -> (*mut Self::Buffer, *const Self::Buffer) {
        let ptr = self.0.get().cast::<T>();

        // Safety: booleans are always 0 or 1 which is always in bounds of an array length 2
        unsafe { (ptr.add(usize::from(which)), ptr.add(usize::from(!which))) }
    }
}

// Safety:
// * the two pointers returned from get are always valid
// * they are disjoint
// * the data is not dereferenced
unsafe impl<T> RawBuffers for SliceRawDoubleBuffer<T> {
    type Buffer = [T];

    fn get(&self, which: bool) -> (*mut Self::Buffer, *const Self::Buffer) {
        let ptr = self.0.get();

        // Safety: scalling slice len doesn't access the data segment of the ptr
        // so there's no data races possible
        let len = unsafe { (*ptr).len() };

        let ptr = ptr.cast::<T>();
        let half = len / 2;

        // Safety: booleans are always 0 or 1 which is always in bounds of an array length 2
        unsafe {
            (
                ptr::slice_from_raw_parts_mut(ptr.add(half * usize::from(which)), half),
                ptr::slice_from_raw_parts(ptr.add(half * usize::from(!which)), half),
            )
        }
    }
}

/// A thread-safe flag
pub struct Flag(core::cell::Cell<bool>);

// SAFETY:
//
// * `load` and `load_unsync` may not mutate the value
//      * `load` and `load_unsync` don't mutate the flag
// * `flip` must switch which the value returned from `load` and `load_unsync`
//      * `flip` flips the boolean flag
/// * `flip` must syncronize with `load`, i.e. all `flip`s must have a happens before relation with `load`
///     * this applies because `Flag` is `!Sync` so program order specifies that all loads and flips are kept in order
unsafe impl Which for Flag {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = Self(core::cell::Cell::new(false));

    fn load(&self) -> bool {
        self.0.get()
    }

    fn flip(&self) -> bool {
        self.0.set(!self.0.get());
        !self.0.get()
    }
}

/// A thread-safe flag
pub struct AtomicFlag(core::sync::atomic::AtomicBool);

// SAFETY:
//
// * `load` and `load_unsync` may not mutate the value
//      * `load` and `load_unsync` don't mutate the flag
// * `flip` must switch which the value returned from `load` and `load_unsync`
//      * `flip` flips the boolean flag
/// * `flip` must syncronize with `load`, i.e. all `flip`s must have a happens before relation with `load`
///     * `flip` uses `Ordering::Release` which syncronizes with `load`'s `Ordering::Acquire` to create a happens before relation
unsafe impl Which for AtomicFlag {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = Self(core::sync::atomic::AtomicBool::new(false));

    unsafe fn load_unsync(&self) -> bool {
        // SAFETY: load unsync guarantees that this read won't race with flip
        unsafe { core::ptr::read(&self.0).into_inner() }
    }

    fn load(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn flip(&self) -> bool {
        self.0.fetch_xor(true, Ordering::Release)
    }
}
