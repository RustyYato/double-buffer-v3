//! core traits and type aliases

use core::ops::Deref;

use crate::raw::Shared;

/// the strong reference type of a given weak reference type
pub type StrongOf<W> = <W as WeakRef>::Strong;
/// the weak reference type of a given strong reference type
pub type WeakOf<S> = <S as StrongRef>::Weak;
/// the strategy type used by a strong reference type
pub type StrategyOf<S> = <S as StrongRef>::Strategy;
/// the raw buffers type used by a strong reference type
pub type RawBuffersOf<S> = <S as StrongRef>::RawBuffers;
/// the buffer type used by the raw buffers type
pub type BufferOf<B> = <B as RawBuffers>::Buffer;
/// the writer tag type of a strategy type
pub type WriterTag<S> = <S as Strategy>::WriterTag;
/// the reader tag type of a strategy type
pub type ReaderTagOf<S> = <S as Strategy>::ReaderTag;
/// the reader guard type of a strategy type
pub type ReaderGuardOf<S> = <S as Strategy>::ReaderGuard;
/// the boolean flag type of a strategy type
pub type WhichOf<S> = <S as Strategy>::Which;
/// the validation token type of a strategy type
pub type ValidationTokenOf<S> = <S as Strategy>::ValidationToken;
/// the validation error type of a strategy type
pub type ValidationErrorOf<S> = <S as Strategy>::ValidationError;
/// the capture type of a strategy type
pub type CaptureOf<S> = <S as Strategy>::Capture;
/// The pause state type for a strategy type
pub type PauseOf<S> = <S as Strategy>::Pause;

/// A conversion trait to a `StrongRef`.
///
/// # Safety
///
/// * the result of `into_strong` must not alias with any other pointer
/// * the shared buffer in `get_mut` must be the same shared buffer returned from `<Self::Strong as Deref>::deref`
pub unsafe trait IntoStrongRef {
    /// The strong reference type being returned
    type Strong: StrongRef;

    /// Get a mutable reference to the shared buffer
    fn get_mut(&mut self) -> &mut Shared<StrategyOf<Self::Strong>, RawBuffersOf<Self::Strong>>;

    /// Creates a strong reference from a value
    fn into_strong(self) -> Self::Strong;
}

/// A strong reference to the underlying shared buffer
///
/// A strong reference will keep the buffers alive
///
/// # Safety
///
/// * `Deref::deref` cannot change which value it points to
/// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
///     `WeakRef::upgrade` returns `Ok`
/// * moving the strong ref shouldn't invalidate pointers to inside the strong ref
pub unsafe trait StrongRef:
    Deref<Target = crate::raw::Shared<Self::Strategy, Self::RawBuffers>>
{
    /// The raw buffers type specified by this strongref
    type RawBuffers: ?Sized + RawBuffers;
    /// The strategy type specified by this strongref
    type Strategy: Strategy;
    /// The associated weak ref type that can be downgraded to
    type Weak: WeakRef<Strong = Self>;

    /// Downgrade to a weak ref
    ///
    /// A weak ref may not keep the data alive, but will keep the memory allocated
    fn downgrade(this: &Self) -> Self::Weak;
}

/// A weak reference to the underlying shared buffer
///
/// A weak reference will keep the buffers allocated, but not necessarily alive
///
/// # Safety
///
/// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
///     `WeakRef::upgrade` returns `Ok`
/// * once `WeakRef::upgrade` returns `Err` it must always return `Err`
pub unsafe trait WeakRef: Clone {
    /// The associated strong reference
    type Strong: StrongRef<Weak = Self>;
    /// The error when upgrading to a strong reference
    type UpgradeError;

    /// Upgrade to a strong ref
    ///
    /// If there are no other strong refs this may return Err
    fn upgrade(this: &Self) -> Result<Self::Strong, Self::UpgradeError>;
}

/// The raw unsyncronized double buffer
///
/// # Safety
///
/// * the two pointers returned from get are valid for reads and writes as long as `Self` is alive
/// * they are disjoint
/// * the data is not dereferenced
pub unsafe trait RawBuffers {
    /// The underlying buffered type
    type Buffer: ?Sized;

    /// get a pointer to the two buffers
    fn get(&self, which: bool) -> (*mut Self::Buffer, *const Self::Buffer);
}

/// The syncronization strategy
///
/// # Safety
///
/// FIXME
pub unsafe trait Strategy {
    /// The writer tag, will be owned by the writer and can identify writers to the strategy
    type WriterTag;
    /// The reader tag, will be owned by the reader and can identify readers to the strategy
    type ReaderTag;
    /// The type of boolean flag to use
    type Which: Which;

    /// The validation token
    type ValidationToken;
    /// A validation error in case the readers can't exit the write buffer
    type ValidationError: core::fmt::Debug;
    /// A capture token which holds which readers are in the write buffer
    type Capture;
    /// The guard type that
    type ReaderGuard;
    /// a type which can be used to add state to pause iterations
    type Pause: Default;

    /// Creates a writer tag managed by this strategy
    ///
    /// # Safety
    ///
    /// FIXME
    unsafe fn create_writer_tag(&mut self) -> Self::WriterTag;

    /// Creates a reader tag managed by this strategy
    ///
    /// # Safety
    ///
    /// the writer tag must be managed by this strategy
    unsafe fn create_reader_tag_from_writer(&self, parent: &Self::WriterTag) -> Self::ReaderTag;

    /// Creates a reader tag managed by this strategy
    ///
    /// # Safety
    ///
    /// the reader tag must be managed by this strategy
    unsafe fn create_reader_tag_from_reader(&self, parent: &Self::ReaderTag) -> Self::ReaderTag;

    /// Creates a reader tag not managed by this strategy out of thin air
    fn dangling_reader_tag() -> Self::ReaderTag;

    /// Check if it's potentially safe to flip the buffers
    fn validate_swap(
        &self,
        writer: &mut Self::WriterTag,
    ) -> Result<Self::ValidationToken, Self::ValidationError>;

    /// Capture the readers that are currently in the writer buffer
    ///
    /// # Safety
    ///
    /// * The validation token must have come from a call to `validate_swap` right before swapping the buffers
    /// * Must poll `have_readers_exited` until it returns true before calling `validate_swap` again
    unsafe fn capture_readers(
        &self,
        writer: &mut Self::WriterTag,
        validation_token: Self::ValidationToken,
    ) -> Self::Capture;

    /// Check if all the readers captured at the specified capture point have exited
    ///
    /// # Safety
    ///
    /// The `WriterTag` and `Capture` should been created by `self`
    /// The `WriterTag` should have been used to create `Capture`
    unsafe fn have_readers_exited(
        &self,
        writer: &Self::WriterTag,
        capture: &mut Self::Capture,
    ) -> bool;

    /// Pause the current thread while waiting for readers to exit
    fn pause(&self, _writer: &Self::WriterTag, _pause: &mut Self::Pause) {}

    /// begin a read guard, this locks the buffer and allows `capture_readers` to see which readers are actively reading
    ///
    /// # Panics
    ///
    /// may panic if `begin_read_guard` is called twice before calling `end_read_guard`
    ///
    /// # Safety
    ///
    /// the reader tag may not be dangling
    unsafe fn begin_read_guard(&self, reader: &mut Self::ReaderTag) -> Self::ReaderGuard;

    /// end the read guard for the given reader
    ///
    /// # Safety
    ///
    /// * the reader must have been created by this strategy
    /// * the reader specified must have created the guard
    unsafe fn end_read_guard(&self, reader: &mut Self::ReaderTag, guard: Self::ReaderGuard);
}

/// A token for which buffer is on top
///
/// # Safety
///
/// * `load` and `load_unsync` may not mutate the value
/// * `flip` must switch which the value returned from `load` and `load_unsync`
/// * `flip` must syncronize with `load`, i.e. all `flip`s must have a happens before relation with `load`
///
///  i.e. this functions shoul always be safe to call and shoule never panic
///
/// ```
/// # use dbuf::interface::Which;
/// fn which<W: Which>(which: W) {
///     let a = which.load();
///     let a_unsync = unsafe { which.load_unsync() };
///     
///     let b = which.flip();
///     
///     let c = which.load();
///     let c_unsync = unsafe { which.load_unsync() };
///     
///     assert_eq!(a, !c);
///     assert_eq!(a, a_unsync);
///     assert_eq!(c, c_unsync);
/// }
/// ```
pub unsafe trait Which: Sized {
    /// The initial value of Self
    ///
    /// Which::INIT.load() should always return false
    const INIT: Self;

    /// INTERNAL ONLY
    #[cfg(feature = "loom")]
    fn new() -> Self {
        Self::INIT
    }

    /// Load the boolean flag for which buffer is on top
    ///
    /// # Safety
    ///
    /// This may not be called in parellel to `Which::flip`
    unsafe fn load_unsync(&self) -> bool {
        self.load()
    }

    /// Load the boolean flag for which buffer is in on top
    fn load(&self) -> bool;

    /// Switch the two buffers
    fn flip(&self);
}

/// A strategy for parking threads
pub trait WaitStrategy {
    /// A value which can be used to store state between subsequent calls to park
    type State: Default;

    /// park the current thread
    ///
    /// returns true if it has saturated (will not park for a longer period of time than the last)
    fn wait(&self, park: &mut Self::State) -> bool;

    /// unpark the one parked thread
    fn notify(&self);
}
