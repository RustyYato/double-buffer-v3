//! Strong and Weak reference implementations

use crate::{
    interface::{IntoStrongRef, RawBuffers, Strategy, StrongRef, WeakRef},
    raw::Shared,
};

#[cfg(feature = "alloc")]
pub mod alloc;

// SAFETY: the result of `into_strong` does not alias with any other pointer
// because `&mut _` doesn't alias with any other pointer
unsafe impl<'a, S: Strategy, B: ?Sized + RawBuffers> IntoStrongRef for &'a mut Shared<S, B> {
    type Strong = &'a Shared<S, B>;

    fn get_mut(
        &mut self,
    ) -> &mut Shared<
        crate::interface::StrategyOf<Self::Strong>,
        crate::interface::RawBuffersOf<Self::Strong>,
    > {
        self
    }

    fn into_strong(self) -> Self::Strong {
        self
    }
}

// SAFETY:
// * `Deref::deref` cannot change which value it points to
// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
//     `WeakRef::upgrade` returns `Ok`
// * moving the strong ref shouldn't invalidate pointers to inside the strong ref
unsafe impl<S: Strategy, B: ?Sized + RawBuffers> StrongRef for &Shared<S, B> {
    type RawBuffers = B;
    type Strategy = S;

    type Weak = Self;

    fn downgrade(this: &Self) -> Self::Weak {
        *this
    }
}

// SAFETY:
// * `WeakRef::upgrade(&StrongRef::downgrade(this))` must alias with `this` if
//     `WeakRef::upgrade` returns `Ok`
// * once `WeakRef::upgrade` returns `Err` it must always return `Err`
unsafe impl<S: Strategy, B: ?Sized + RawBuffers> WeakRef for &Shared<S, B> {
    type Strong = Self;
    type UpgradeError = core::convert::Infallible;

    fn upgrade(this: &Self) -> Result<Self::Strong, Self::UpgradeError> {
        Ok(*this)
    }
}
