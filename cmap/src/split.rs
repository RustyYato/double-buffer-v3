use std::{
    borrow::Borrow,
    fmt,
    hash::Hash,
    ops::Deref,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

pub trait Split {
    fn split(&mut self) -> Self;
}

impl<T: Clone> Split for T {
    #[inline]
    fn split(&mut self) -> Self {
        self.clone()
    }
}

pub struct Pair<T: ?Sized> {
    ptr: NonNull<PairInner<T>>,
}

struct PairInner<T: ?Sized> {
    has_other: AtomicBool,
    value: T,
}

unsafe impl<T: Send + Sync + ?Sized> Send for Pair<T> {}
unsafe impl<T: Send + Sync + ?Sized> Sync for Pair<T> {}

impl<T: ?Sized> Deref for Pair<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &self.ptr.as_ref().value }
    }
}

impl<T> Pair<T> {
    pub fn new(value: T) -> Self {
        Self {
            ptr: unsafe {
                NonNull::new_unchecked(Box::into_raw(Box::new(PairInner {
                    has_other: AtomicBool::new(false),
                    value,
                })))
            },
        }
    }
}

impl<T> Split for Pair<T> {
    fn split(&mut self) -> Self {
        let result = unsafe { self.ptr.as_ref() }.has_other.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Acquire,
        );

        assert!(result.is_ok(), "Cannot split a pair more than once");

        Self { ptr: self.ptr }
    }
}

impl<T: ?Sized> Drop for Pair<T> {
    fn drop(&mut self) {
        if unsafe { self.ptr.as_ref() }
            .has_other
            .swap(false, Ordering::Release)
        {
            return;
        }

        unsafe { Box::from_raw(self.ptr.as_ptr()) };
    }
}

#[test]
fn split_once() {
    let mut pair = Pair::new(10);

    let mut b = pair.split();

    drop(pair);

    let _a = b.split();
}

#[test]
#[should_panic = "Cannot split a pair more than once"]
fn split_multiple() {
    let mut pair = Pair::new(10);
    let _b = pair.split();
    let _c = pair.split();
}

impl<T: ?Sized + Eq> Eq for Pair<T> {}
impl<T: ?Sized + PartialEq> PartialEq<T> for Pair<T> {
    #[inline]
    fn eq(&self, other: &T) -> bool {
        T::eq(self, other)
    }
}
impl<T: ?Sized + PartialEq> PartialEq<Pair<T>> for Pair<T> {
    #[inline]
    fn eq(&self, other: &Pair<T>) -> bool {
        T::eq(self, other)
    }
}

impl<T: ?Sized + PartialOrd> PartialOrd<T> for Pair<T> {
    #[inline]
    fn partial_cmp(&self, other: &T) -> Option<std::cmp::Ordering> {
        T::partial_cmp(self, other)
    }
}
impl<T: ?Sized + PartialOrd> PartialOrd<Pair<T>> for Pair<T> {
    #[inline]
    fn partial_cmp(&self, other: &Pair<T>) -> Option<std::cmp::Ordering> {
        T::partial_cmp(self, other)
    }
}

impl<T: ?Sized + Ord> Ord for Pair<T> {
    #[inline]
    fn cmp(&self, other: &Pair<T>) -> std::cmp::Ordering {
        T::cmp(self, other)
    }
}

impl<T: ?Sized + Hash> Hash for Pair<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        T::hash(self, state)
    }
}

impl<T: ?Sized> Borrow<T> for Pair<T> {
    fn borrow(&self) -> &T {
        self
    }
}

impl<T: Borrow<str>> Borrow<str> for Pair<T> {
    fn borrow(&self) -> &str {
        T::borrow(self)
    }
}

impl<T: Borrow<[U]>, U> Borrow<[U]> for Pair<T> {
    fn borrow(&self) -> &[U] {
        T::borrow(self)
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for Pair<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        T::fmt(self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for Pair<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        T::fmt(self, f)
    }
}
