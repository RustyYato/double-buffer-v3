//! core traits and type aliases

use core::ops::Deref;

/// the strong reference type of a given weak reference type
pub type StrongOf<W> = <W as WeakRef>::Strong;
/// the weak reference type of a given strong reference type
pub type WeakOf<S> = <S as StrongRef>::Weak;
/// the strategy type used by a strong reference type
pub type StrategyOf<S> = <S as StrongRef>::Strategy;
/// the raw buffers type used by a strong reference type
pub type RawBuffersOf<S> = <S as StrongRef>::RawBuffers;
/// the buffer type used by the raw buffers type
pub type Buffer<B> = <B as RawBuffers>::Buffer;
/// the writer tag type of a strategy type
pub type WriterTag<S> = <S as Strategy>::WriterTag;
/// the reader tag type of a strategy type
pub type ReaderTag<S> = <S as Strategy>::ReaderTag;
/// the boolean flag type of a strategy type
pub type WhichOf<S> = <S as Strategy>::Which;

/// A conversion trait to a `StrongRef`.
///
/// # Safety
///
/// the result of `into_strong` must not alias with any other pointer
pub unsafe trait IntoStrongRef {
    /// The strong reference type being returned
    type Strong: StrongRef;

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
pub unsafe trait StrongRef:
    Deref<Target = crate::raw::Shared<Self::Strategy, Self::RawBuffers>>
{
    /// The raw buffers type specified by this strongref
    type RawBuffers: RawBuffers;
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
/// * `Deref::deref` cannot change which value it points to
/// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
///     `WeakRef::upgrade` returns `Ok`
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
/// * the two pointers returned from get are always valid
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

    /// Creates a writer tag managed by this strategy
    ///
    /// # Safety
    ///
    /// FIXME
    unsafe fn create_writer_tag(&self) -> Self::WriterTag;

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
pub unsafe trait Which {
    /// The initial value of Self
    ///
    /// Which::INIT.load() should always return false
    const INIT: Self;

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
    ///
    /// returns the old value of the buffers
    fn flip(&self) -> bool;
}
