use self::ordbag::OrdBag;

use super::{DefaultHasher, DefaultStrat};
use std::{borrow::Borrow, collections::BTreeMap, convert::Infallible, fmt, ops::Deref};

use dbuf::interface::Strategy;
use sync_wrapper::SyncWrapper;

use crate::split::Split;

pub mod ordbag;

pub struct Bag<T> {
    inner: BagInner<T>,
}

impl<T> Default for Bag<T> {
    fn default() -> Self {
        Self {
            inner: BagInner::One(None),
        }
    }
}

impl<T> Bag<T> {
    fn get_one(&self) -> Option<&T> {
        match &self.inner {
            BagInner::One(None) => None,
            BagInner::One(Some((inner, _))) => Some(inner),
            BagInner::Many(many) => many.iter().next(),
        }
    }

    pub fn iter(&self) -> BagIter<'_, T> {
        self.into_iter()
    }
}

impl<T: Ord> Bag<T> {
    fn insert(&mut self, value: T) {
        match self.inner {
            BagInner::One(None) => self.inner = BagInner::One(Some((value, 1))),
            BagInner::One(Some((ref inner, ref mut count))) if *inner == value => *count += 1,
            BagInner::One(Some(_)) => {
                let (inner, count) = match core::mem::take(self).inner {
                    BagInner::One(Some((value, count))) => (value, count),
                    _ => unreachable!(),
                };
                self.inner = BagInner::One(None);
                let mut bag = OrdBag::new();
                bag.insert_many(inner, count);
                bag.insert(value);
                self.inner = BagInner::Many(bag);
            }
            BagInner::Many(ref mut bag) => {
                bag.insert(value);
            }
        }
    }

    fn remove(&mut self, value: &T) {
        match self.inner {
            BagInner::One(Some((ref inner, ref mut count))) if inner == value && *count > 0 => {
                *count -= 1
            }
            BagInner::One(_) => (),
            BagInner::Many(ref mut bag) => {
                bag.remove(value);
            }
        }
    }

    fn is_empty(&self) -> bool {
        match &self.inner {
            BagInner::One(None) | BagInner::One(Some((_, 0))) => true,
            BagInner::One(Some(_)) => false,
            BagInner::Many(bag) => bag.is_empty(),
        }
    }

    fn retain<F: FnMut(&T, usize) -> usize>(&mut self, mut f: F) {
        match self.inner {
            BagInner::One(None) => (),
            BagInner::One(Some((ref value, ref mut count))) => {
                *count = f(value, *count);
            }
            BagInner::Many(ref mut bag) => bag.retain(f),
        }
    }
}

enum BagInner<T> {
    One(Option<(T, usize)>),
    Many(OrdBag<T>),
}

pub struct CBTreeMultiMap<K, V = DefaultHasher, Strat = DefaultStrat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::op::OpWriter<
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::SizedRawDoubleBuffer<BTreeMap<K, Bag<V>>>>,
        MapOp<K, V>,
    >,
}

pub struct CBTreeMultiMapReader<K, V = DefaultHasher, Strat = DefaultStrat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::Reader<
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::SizedRawDoubleBuffer<BTreeMap<K, Bag<V>>>>,
    >,
}

pub struct CBTreeMapReadGuard<'a, K, V, Strat = DefaultStrat, T: ?Sized = BTreeMap<K, Bag<V>>>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::ReadGuard<
        'a,
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::SizedRawDoubleBuffer<BTreeMap<K, Bag<V>>>>,
        T,
    >,
}

pub enum MapOp<K, V> {
    Insert(K, V),
    Clear(K),
    Remove(K, V),
    #[allow(clippy::type_complexity)]
    Arbitrary(SyncWrapper<Box<dyn FnMut(bool, &mut BTreeMap<K, Bag<V>>) + Send>>),
    Purge,
}

impl<K, V> dbuf::op_log::Operation<BTreeMap<K, Bag<V>>> for MapOp<K, V>
where
    K: Ord + Split,
    V: Split + Ord,
{
    fn apply(&mut self, buffer: &mut BTreeMap<K, Bag<V>>) {
        match self {
            MapOp::Insert(key, value) => {
                buffer
                    .entry(key.split())
                    .or_insert_with(Bag::default)
                    .insert(value.split());
            }
            MapOp::Clear(key) => {
                buffer.remove(key);
            }
            MapOp::Remove(key, value) => match buffer.get_mut(key) {
                Some(bag) => {
                    bag.remove(value);
                }
                None => (),
            },
            MapOp::Arbitrary(f) => f.get_mut()(false, buffer),
            MapOp::Purge => buffer.clear(),
        }
    }

    fn apply_last(self, buffer: &mut BTreeMap<K, Bag<V>>) {
        match self {
            MapOp::Insert(key, value) => {
                buffer.entry(key).or_insert_with(Bag::default).insert(value);
            }
            MapOp::Clear(key) => {
                buffer.remove(&key);
            }
            MapOp::Remove(key, value) => match buffer.get_mut(&key) {
                Some(bag) => {
                    bag.remove(&value);
                }
                None => (),
            },
            MapOp::Arbitrary(mut f) => f.get_mut()(false, buffer),
            MapOp::Purge => buffer.clear(),
        }
    }
}

impl<K, V> CBTreeMultiMap<K, V> {
    pub fn new() -> Self {
        Self::from_maps(BTreeMap::new(), BTreeMap::new())
    }
}

impl<K, V, Strat> Default for CBTreeMultiMap<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible> + Default,
{
    fn default() -> Self {
        Self::from_maps(Default::default(), Default::default())
    }
}

impl<K, V, Strat> CBTreeMultiMap<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible> + Default,
{
    pub fn from_maps(front: BTreeMap<K, Bag<V>>, back: BTreeMap<K, Bag<V>>) -> Self {
        Self::from_raw_parts(front, back, Strat::default())
    }
}

impl<K, V, Strat> CBTreeMultiMap<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn from_raw_parts(
        front: BTreeMap<K, Bag<V>>,
        back: BTreeMap<K, Bag<V>>,
        strategy: Strat,
    ) -> Self {
        Self {
            inner: dbuf::op::OpWriter::from(dbuf::raw::Writer::new(dbuf::ptrs::alloc::Owned::new(
                dbuf::raw::Shared::from_raw_parts(
                    strategy,
                    dbuf::raw::SizedRawDoubleBuffer::new(front, back),
                ),
            ))),
        }
    }

    pub fn reader(&self) -> CBTreeMultiMapReader<K, V, Strat> {
        CBTreeMultiMapReader {
            inner: self.inner.reader(),
        }
    }

    pub fn load(&self) -> &BTreeMap<K, Bag<V>> {
        self.inner.split().reader
    }
}

impl<K, V, Strat> CBTreeMultiMap<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
    K: Ord + Split,
    V: Split + Ord,
{
    pub fn insert(&mut self, key: K, value: V) {
        self.inner.apply(MapOp::Insert(key, value));
    }

    pub fn remove(&mut self, key: K, value: V) {
        self.inner.apply(MapOp::Remove(key, value));
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&Bag<V>>
    where
        Q: ?Sized + Ord,
        K: Borrow<Q>,
    {
        self.inner.split().reader.get(key)
    }

    pub fn get_one<Q>(&self, key: &Q) -> Option<&V>
    where
        Q: ?Sized + Ord,
        K: Borrow<Q>,
    {
        self.inner.split().reader.get(key)?.get_one()
    }

    pub fn purge(&mut self) {
        self.inner.apply(MapOp::Purge)
    }

    pub fn clear(&mut self, key: K) {
        self.inner.apply(MapOp::Clear(key))
    }

    pub fn retain(&mut self, mut f: impl FnMut(bool, &K, &V) -> bool + Send + 'static) {
        self.inner.apply(MapOp::Arbitrary(SyncWrapper::new(Box::new(
            move |is_first, map| {
                map.retain(|k, v| {
                    v.retain(|v, mut count| {
                        #[allow(clippy::mut_range_bound)]
                        for _ in 0..count {
                            count -= usize::from(f(is_first, k, v))
                        }
                        count
                    });
                    !v.is_empty()
                })
            },
        ))))
    }

    pub fn unapplied(&self) -> &[MapOp<K, V>] {
        self.inner.unapplied()
    }

    pub fn refresh(&mut self) {
        self.inner.swap_buffers();
    }

    pub fn flush(&mut self) {
        self.inner.flush()
    }
}

impl<K, V, Strat> Clone for CBTreeMultiMapReader<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<K, V, Strat> CBTreeMultiMapReader<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn load(&mut self) -> CBTreeMapReadGuard<K, V, Strat> {
        CBTreeMapReadGuard {
            inner: self.inner.get(),
        }
    }

    pub fn get<Q>(&mut self, key: &Q) -> Option<CBTreeMapReadGuard<K, V, Strat, Bag<V>>>
    where
        Q: ?Sized + Ord,
        K: Ord + Borrow<Q>,
    {
        self.load().try_map(|map| map.get(key)).ok()
    }

    pub fn get_one<Q>(&mut self, key: &Q) -> Option<CBTreeMapReadGuard<K, V, Strat, V>>
    where
        Q: ?Sized + Ord,
        K: Ord + Borrow<Q>,
    {
        let guard = self.get(key)?;

        CBTreeMapReadGuard::try_map(guard, Bag::get_one).ok()
    }
}

impl<K, V, Strat, T: ?Sized> Deref for CBTreeMapReadGuard<'_, K, V, Strat, T>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, K, V, Strat, T: ?Sized> CBTreeMapReadGuard<'a, K, V, Strat, T>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn map<U: ?Sized>(
        self,
        f: impl FnOnce(&T) -> &U,
    ) -> CBTreeMapReadGuard<'a, K, V, Strat, U> {
        CBTreeMapReadGuard {
            inner: dbuf::raw::ReadGuard::map(self.inner, f),
        }
    }

    pub fn try_map<U: ?Sized>(
        self,
        f: impl FnOnce(&T) -> Option<&U>,
    ) -> Result<CBTreeMapReadGuard<'a, K, V, Strat, U>, Self> {
        match dbuf::raw::ReadGuard::try_map(self.inner, f) {
            Ok(inner) => Ok(CBTreeMapReadGuard { inner }),
            Err(inner) => Err(CBTreeMapReadGuard { inner }),
        }
    }
}

impl<K, V, Strat, T: ?Sized + core::fmt::Debug> core::fmt::Debug
    for CBTreeMapReadGuard<'_, K, V, Strat, T>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        T::fmt(self, f)
    }
}

impl<'a, T> IntoIterator for &'a Bag<T> {
    type Item = &'a T;
    type IntoIter = BagIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        match &self.inner {
            BagInner::One(None) => BagIter::One(None),
            BagInner::One(Some((value, count))) => BagIter::One(Some((value, *count))),
            BagInner::Many(many) => BagIter::Many(many.iter()),
        }
    }
}

pub enum BagIter<'a, T> {
    One(Option<(&'a T, usize)>),
    Many(ordbag::Iter<'a, T>),
}

impl<'a, T> Iterator for BagIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            BagIter::One(None) | BagIter::One(Some((_, 0))) => None,
            BagIter::One(Some((value, count))) => {
                *count -= 1;
                Some(value)
            }
            BagIter::Many(many) => many.next(),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Bag<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self).finish()
    }
}
