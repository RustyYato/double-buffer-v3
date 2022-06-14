//! ptrs that need to allocate

use core::ops::Deref;
#[cfg(feature = "loom")]
use loom::sync::Arc;
use std::rc::{Rc, Weak};
#[cfg(not(feature = "loom"))]
use std::sync::{Arc, Weak as AWeak};

use crate::{
    interface::{IntoStrongRef, RawBuffers, Strategy, StrongRef, WeakRef, WhichOf},
    raw::Shared,
};

/// The default strategy used for from_buffers
type DefaultStrategy = crate::strategy::HazardStrategy;
/// The default strategy used for from_buffers
type LocalDefaultStrategy = crate::strategy::LocalHazardStrategy;

/// An unique owned strong ptr to a double buffer
#[cfg(not(feature = "loom"))]
pub struct OwnedWithWeak<S, B, W = WhichOf<S>>(Arc<Shared<S, B, W>>);

#[cfg(not(feature = "loom"))]
impl<S: Strategy, B: RawBuffers> OwnedWithWeak<S, B> {
    /// create a new owned ptr
    pub fn new(shared: Shared<S, B>) -> Self {
        Self(Arc::new(shared))
    }
}

#[cfg(feature = "alloc")]
#[cfg(not(feature = "loom"))]
impl<B> OwnedWithWeak<DefaultStrategy, crate::raw::SizedRawDoubleBuffer<B>> {
    /// create a new owned ptr
    pub fn from_buffers(front: B, back: B) -> Self {
        Self::new(Shared::new(
            DefaultStrategy::new(),
            crate::raw::SizedRawDoubleBuffer::new(front, back),
        ))
    }
}

#[cfg(not(feature = "loom"))]
impl<S, B, W> TryFrom<Arc<Shared<S, B, W>>> for OwnedWithWeak<S, B, W> {
    type Error = Arc<Shared<S, B, W>>;

    fn try_from(mut value: Arc<Shared<S, B, W>>) -> Result<Self, Self::Error> {
        if Arc::get_mut(&mut value).is_some() {
            Ok(Self(value))
        } else {
            Err(value)
        }
    }
}

// SAFETY:
//
// * the result of `into_strong` must not alias with any other pointer
// * the shared buffer in `get_mut` must be the same shared buffer returned from `<Self::Strong as Deref>::deref`
#[cfg(not(feature = "loom"))]
unsafe impl<S: Strategy, B: RawBuffers> IntoStrongRef for OwnedWithWeak<S, B> {
    type Strong = OwnedStrong<S, B>;

    fn get_mut(
        &mut self,
    ) -> &mut Shared<
        crate::interface::StrategyOf<Self::Strong>,
        crate::interface::RawBuffersOf<Self::Strong>,
    > {
        // SAFETY: We have unique access to this Arc and `as_ptr` can't drop
        // mut provenance because the following snippet is guaranteed
        // to be safe by the docs on `Arc`
        //
        // ```
        // let mut a: Arc<...> = ...;
        // assert!(Arc::get_mut(&mut a).is_some());
        // let ptr = Arc::as_ptr(&a);
        // let mut a = ManuallyDrop::new(Arc::from_raw(ptr));
        // // this would create a mutable reference derived from `ptr`
        // assert!(Arc::get_mut(&mut a).is_some());
        // ```
        unsafe { &mut *(Arc::as_ptr(&self.0) as *mut Shared<S, B>) }
    }

    fn into_strong(self) -> Self::Strong {
        OwnedStrong(self.0)
    }
}

/// An owned strong ptr to a shared double buffer
#[cfg(not(feature = "loom"))]
pub struct OwnedStrong<S, B, W = WhichOf<S>>(Arc<Shared<S, B, W>>);
/// An owned weak ptr to a shared double buffer
#[cfg(not(feature = "loom"))]
pub struct OwnedWeak<S, B, W = WhichOf<S>>(AWeak<Shared<S, B, W>>);

/// The error representing a failed upgrade from OwnedWeak to OwnedStrong
#[cfg(not(feature = "loom"))]
pub struct UpgradeError;

#[cfg(not(feature = "loom"))]
impl core::fmt::Debug for UpgradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("could upgrade OwnedWeak to OwnedStrong")
    }
}

#[cfg(not(feature = "loom"))]
impl<S, B, W> Deref for OwnedStrong<S, B, W> {
    type Target = Shared<S, B, W>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(not(feature = "loom"))]
impl<S, B, W> Clone for OwnedWeak<S, B, W> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

// SAFETY:
//
// * `Deref::deref` cannot change which value it points to
// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
//     `WeakRef::upgrade` returns `Ok`
/// * moving the strong ref shouldn't invalidate pointers to inside the strong ref
#[cfg(not(feature = "loom"))]
unsafe impl<S: Strategy, B: RawBuffers> StrongRef for OwnedStrong<S, B> {
    type RawBuffers = B;
    type Strategy = S;
    type Weak = OwnedWeak<S, B>;

    fn downgrade(this: &Self) -> Self::Weak {
        OwnedWeak(Arc::downgrade(&this.0))
    }
}

// SAFETY:
//
/// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
///     `WeakRef::upgrade` returns `Ok`
/// * once `WeakRef::upgrade` returns `Err` it must always return `Err`
#[cfg(not(feature = "loom"))]
unsafe impl<S: Strategy, B: RawBuffers> WeakRef for OwnedWeak<S, B> {
    type Strong = OwnedStrong<S, B>;
    type UpgradeError = UpgradeError;

    fn upgrade(this: &Self) -> Result<Self::Strong, Self::UpgradeError> {
        AWeak::upgrade(&this.0).ok_or(UpgradeError).map(OwnedStrong)
    }
}

/// An unique owned strong ptr to a double buffer
pub struct LocalOwnedWithWeak<S, B, W = WhichOf<S>>(Rc<Shared<S, B, W>>);

impl<S: Strategy, B: RawBuffers> LocalOwnedWithWeak<S, B> {
    /// create a new owned ptr
    pub fn new(shared: Shared<S, B>) -> Self {
        Self(Rc::new(shared))
    }
}

#[cfg(feature = "alloc")]
impl<B> LocalOwnedWithWeak<LocalDefaultStrategy, crate::raw::SizedRawDoubleBuffer<B>> {
    /// create a new LocalOwned ptr
    pub fn from_buffers(front: B, back: B) -> Self {
        Self::new(Shared::new(
            LocalDefaultStrategy::new(),
            crate::raw::SizedRawDoubleBuffer::new(front, back),
        ))
    }
}

impl<S, B, W> TryFrom<Rc<Shared<S, B, W>>> for LocalOwnedWithWeak<S, B, W> {
    type Error = Rc<Shared<S, B, W>>;

    fn try_from(mut value: Rc<Shared<S, B, W>>) -> Result<Self, Self::Error> {
        if Rc::get_mut(&mut value).is_some() {
            Ok(Self(value))
        } else {
            Err(value)
        }
    }
}

// SAFETY:
//
// * the result of `into_strong` must not alias with any other pointer
// * the shared buffer in `get_mut` must be the same shared buffer returned from `<Self::Strong as Deref>::deref`
unsafe impl<S: Strategy, B: RawBuffers> IntoStrongRef for LocalOwnedWithWeak<S, B> {
    type Strong = LocalOwnedStrong<S, B>;

    fn get_mut(
        &mut self,
    ) -> &mut Shared<
        crate::interface::StrategyOf<Self::Strong>,
        crate::interface::RawBuffersOf<Self::Strong>,
    > {
        // SAFETY: We have unique access to this Arc and `as_ptr` can't drop
        // mut provenance because the following snippet is guaranteed
        // to be safe by the docs on `Arc`
        //
        // ```
        // let mut a: Arc<...> = ...;
        // assert!(Arc::get_mut(&mut a).is_some());
        // let ptr = Arc::as_ptr(&a);
        // let mut a = ManuallyDrop::new(Arc::from_raw(ptr));
        // // this would create a mutable reference derived from `ptr`
        // assert!(Arc::get_mut(&mut a).is_some());
        // ```
        unsafe { &mut *(Rc::as_ptr(&self.0) as *mut Shared<S, B>) }
    }

    fn into_strong(self) -> Self::Strong {
        LocalOwnedStrong(self.0)
    }
}

/// An owned strong ptr to a shared double buffer
pub struct LocalOwnedStrong<S, B, W = WhichOf<S>>(Rc<Shared<S, B, W>>);
/// An owned weak ptr to a shared double buffer
pub struct LocalOwnedWeak<S, B, W = WhichOf<S>>(Weak<Shared<S, B, W>>);

/// The error representing a failed upgrade from LocalOwnedWeak to LocalOwnedStrong
pub struct LocalUpgradeError;

impl core::fmt::Debug for LocalUpgradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("could upgrade LocalOwnedWeak to LocalOwnedStrong")
    }
}

impl<S, B, W> Deref for LocalOwnedStrong<S, B, W> {
    type Target = Shared<S, B, W>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S, B, W> Clone for LocalOwnedWeak<S, B, W> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

// SAFETY:
//
// * `Deref::deref` cannot change which value it points to
// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
//     `WeakRef::upgrade` returns `Ok`
/// * moving the strong ref shouldn't invalidate pointers to inside the strong ref
unsafe impl<S: Strategy, B: RawBuffers> StrongRef for LocalOwnedStrong<S, B> {
    type RawBuffers = B;
    type Strategy = S;
    type Weak = LocalOwnedWeak<S, B>;

    fn downgrade(this: &Self) -> Self::Weak {
        LocalOwnedWeak(Rc::downgrade(&this.0))
    }
}

// SAFETY:
//
/// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
///     `WeakRef::upgrade` returns `Ok`
/// * once `WeakRef::upgrade` returns `Err` it must always return `Err`
unsafe impl<S: Strategy, B: RawBuffers> WeakRef for LocalOwnedWeak<S, B> {
    type Strong = LocalOwnedStrong<S, B>;
    type UpgradeError = LocalUpgradeError;

    fn upgrade(this: &Self) -> Result<Self::Strong, Self::UpgradeError> {
        Weak::upgrade(&this.0)
            .ok_or(LocalUpgradeError)
            .map(LocalOwnedStrong)
    }
}

/// An unique owned strong ptr to a double buffer
pub struct Owned<S, B, W = WhichOf<S>>(Arc<Shared<S, B, W>>);

impl<S: Strategy, B: RawBuffers> Owned<S, B> {
    /// create a new owned ptr
    pub fn new(shared: Shared<S, B>) -> Self {
        Self(Arc::new(shared))
    }
}

#[cfg(feature = "alloc")]
impl<B> Owned<DefaultStrategy, crate::raw::SizedRawDoubleBuffer<B>> {
    /// create a new owned ptr
    pub fn from_buffers(front: B, back: B) -> Self {
        Self::new(Shared::new(
            DefaultStrategy::new(),
            crate::raw::SizedRawDoubleBuffer::new(front, back),
        ))
    }
}

impl<S, B, W> TryFrom<Arc<Shared<S, B, W>>> for Owned<S, B, W> {
    type Error = Arc<Shared<S, B, W>>;

    fn try_from(mut value: Arc<Shared<S, B, W>>) -> Result<Self, Self::Error> {
        if Arc::get_mut(&mut value).is_some() {
            Ok(Self(value))
        } else {
            Err(value)
        }
    }
}

// SAFETY:
//
// * the result of `into_strong` must not alias with any other pointer
// * the shared buffer in `get_mut` must be the same shared buffer returned from `<Self::Strong as Deref>::deref`
unsafe impl<S: Strategy, B: RawBuffers> IntoStrongRef for Owned<S, B> {
    type Strong = OwnedPtr<S, B>;

    fn get_mut(
        &mut self,
    ) -> &mut Shared<
        crate::interface::StrategyOf<Self::Strong>,
        crate::interface::RawBuffersOf<Self::Strong>,
    > {
        // SAFETY: We have unique access to this Arc and `as_ptr` can't drop
        // mut provenance because the following snippet is guaranteed
        // to be safe by the docs on `Arc`
        //
        // ```
        // let mut a: Arc<...> = ...;
        // assert!(Arc::get_mut(&mut a).is_some());
        // let ptr = Arc::as_ptr(&a);
        // let mut a = ManuallyDrop::new(Arc::from_raw(ptr));
        // // this would create a mutable reference derived from `ptr`
        // assert!(Arc::get_mut(&mut a).is_some());
        // ```
        unsafe { &mut *(Arc::as_ptr(&self.0) as *mut Shared<S, B>) }
    }

    fn into_strong(self) -> Self::Strong {
        OwnedPtr(self.0)
    }
}

/// An owned shared ptr to a shared double buffer
pub struct OwnedPtr<S, B, W = WhichOf<S>>(Arc<Shared<S, B, W>>);

impl<S, B, W> Deref for OwnedPtr<S, B, W> {
    type Target = Shared<S, B, W>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S, B, W> Clone for OwnedPtr<S, B, W> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

// SAFETY:
//
// * `Deref::deref` cannot change which value it points to
// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
//     `WeakRef::upgrade` returns `Ok`
/// * moving the strong ref shouldn't invalidate pointers to inside the strong ref
unsafe impl<S: Strategy, B: RawBuffers> StrongRef for OwnedPtr<S, B> {
    type RawBuffers = B;
    type Strategy = S;
    type Weak = Self;

    fn downgrade(this: &Self) -> Self::Weak {
        OwnedPtr(Arc::clone(&this.0))
    }
}

// SAFETY:
//
/// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
///     `WeakRef::upgrade` returns `Ok`
/// * once `WeakRef::upgrade` returns `Err` it must always return `Err`
unsafe impl<S: Strategy, B: RawBuffers> WeakRef for OwnedPtr<S, B> {
    type Strong = Self;
    type UpgradeError = core::convert::Infallible;

    fn upgrade(this: &Self) -> Result<Self::Strong, Self::UpgradeError> {
        Ok(Self::clone(this))
    }
}

/// An unique LocalOwned strong ptr to a double buffer
pub struct LocalOwned<S, B, W = WhichOf<S>>(Rc<Shared<S, B, W>>);

impl<S: Strategy, B: RawBuffers> LocalOwned<S, B> {
    /// create a new LocalOwned ptr
    pub fn new(shared: Shared<S, B>) -> Self {
        Self(Rc::new(shared))
    }
}

#[cfg(feature = "alloc")]
impl<B> LocalOwned<LocalDefaultStrategy, crate::raw::SizedRawDoubleBuffer<B>> {
    /// create a new LocalOwned ptr
    pub fn from_buffers(front: B, back: B) -> Self {
        Self::new(Shared::new(
            LocalDefaultStrategy::new(),
            crate::raw::SizedRawDoubleBuffer::new(front, back),
        ))
    }
}

impl<S, B, W> TryFrom<Rc<Shared<S, B, W>>> for LocalOwned<S, B, W> {
    type Error = Rc<Shared<S, B, W>>;

    fn try_from(mut value: Rc<Shared<S, B, W>>) -> Result<Self, Self::Error> {
        if Rc::get_mut(&mut value).is_some() {
            Ok(Self(value))
        } else {
            Err(value)
        }
    }
}

// SAFETY:
//
// * the result of `into_strong` must not alias with any other pointer
// * the shared buffer in `get_mut` must be the same shared buffer returned from `<Self::Strong as Deref>::deref`
unsafe impl<S: Strategy, B: RawBuffers> IntoStrongRef for LocalOwned<S, B> {
    type Strong = LocalOwnedPtr<S, B>;

    fn get_mut(
        &mut self,
    ) -> &mut Shared<
        crate::interface::StrategyOf<Self::Strong>,
        crate::interface::RawBuffersOf<Self::Strong>,
    > {
        // SAFETY: We have unique access to this Rc and `as_ptr` can't drop
        // mut provenance because the following snippet is guaranteed
        // to be safe by the docs on `Rc`
        //
        // ```
        // let mut a: Rc<...> = ...;
        // assert!(Rc::get_mut(&mut a).is_some());
        // let ptr = Rc::as_ptr(&a);
        // let mut a = ManuallyDrop::new(Rc::from_raw(ptr));
        // // this would create a mutable reference derived from `ptr`
        // assert!(Rc::get_mut(&mut a).is_some());
        // ```
        unsafe { &mut *(Rc::as_ptr(&self.0) as *mut Shared<S, B>) }
    }

    fn into_strong(self) -> Self::Strong {
        LocalOwnedPtr(self.0)
    }
}

/// An LocalOwned shared ptr to a shared double buffer
pub struct LocalOwnedPtr<S, B, W = WhichOf<S>>(Rc<Shared<S, B, W>>);

impl<S, B, W> Deref for LocalOwnedPtr<S, B, W> {
    type Target = Shared<S, B, W>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S, B, W> Clone for LocalOwnedPtr<S, B, W> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

// SAFETY:
//
// * `Deref::deref` cannot change which value it points to
// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
//     `WeakRef::upgrade` returns `Ok`
/// * moving the strong ref shouldn't invalidate pointers to inside the strong ref
unsafe impl<S: Strategy, B: RawBuffers> StrongRef for LocalOwnedPtr<S, B> {
    type RawBuffers = B;
    type Strategy = S;
    type Weak = Self;

    fn downgrade(this: &Self) -> Self::Weak {
        Self::clone(this)
    }
}

// SAFETY:
//
/// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
///     `WeakRef::upgrade` returns `Ok`
/// * once `WeakRef::upgrade` returns `Err` it must always return `Err`
unsafe impl<S: Strategy, B: RawBuffers> WeakRef for LocalOwnedPtr<S, B> {
    type Strong = Self;
    type UpgradeError = core::convert::Infallible;

    fn upgrade(this: &Self) -> Result<Self::Strong, Self::UpgradeError> {
        Ok(Self::clone(this))
    }
}

#[test]
#[cfg(feature = "std")]
#[cfg(not(feature = "loom"))]
fn test_op_writer() {
    enum Op {
        Add(i32),
        Mul(i32),
    }

    impl crate::op_log::Operation<i32> for Op {
        fn apply(&mut self, buffer: &mut i32) {
            match self {
                Op::Add(a) => *buffer += *a,
                Op::Mul(a) => *buffer *= *a,
            }
        }
    }

    let shared = OwnedWithWeak::from_buffers(0, 0);
    let writer = crate::raw::Writer::new(shared);
    let mut writer = crate::op::OpWriter::from(writer);

    writer.swap_buffers();

    let mut reader = writer.reader();

    assert_eq!(*reader.try_get().unwrap(), 0);

    writer.apply(Op::Add(10));
    writer.apply(Op::Mul(10));

    assert_eq!(*reader.try_get().unwrap(), 0);
    assert_eq!(*writer.split().writer, 0);
    assert_eq!(*writer.split().reader, 0);

    writer.swap_buffers();
    writer.apply(Op::Add(10));

    let mut reader2 = reader.clone();
    let guard = reader2.try_get().unwrap();

    assert_eq!(*reader.try_get().unwrap(), 100);
    assert_eq!(*guard, 100);
    assert_eq!(*writer.split().writer, 0);
    assert_eq!(*writer.split().reader, 100);

    writer.swap_buffers();

    assert_eq!(*reader.try_get().unwrap(), 110);
    assert_eq!(*guard, 100);
    assert!(!core::ptr::eq::<i32>(&*guard, &*reader.try_get().unwrap()));
    assert_eq!(*writer.split().writer, 100);
    assert_eq!(*writer.split().reader, 110);
}
