use super::{DefaultHasher, DefaultStrat};
use std::{
    borrow::Borrow,
    collections::{hash_map::Entry, HashMap},
    convert::Infallible,
    fmt,
    hash::{BuildHasher, Hash},
    ops::Deref,
};

use dbuf::interface::Strategy;
use hashbag::HashBag;
use sync_wrapper::SyncWrapper;

use crate::split::Split;

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
    pub fn get_one(&self) -> Option<&T> {
        match &self.inner {
            BagInner::One(None) => None,
            BagInner::One(Some((inner, _))) => Some(inner),
            BagInner::Many(many) => many.iter().next(),
        }
    }

    pub fn iter(&self) -> BagIter<'_, T> {
        self.into_iter()
    }

    pub fn is_empty(&self) -> bool {
        match &self.inner {
            BagInner::One(None) | BagInner::One(Some((_, 0))) => true,
            BagInner::One(Some(_)) => false,
            BagInner::Many(bag) => bag.is_empty(),
        }
    }

    pub fn len(&self) -> usize {
        match &self.inner {
            BagInner::One(None) => 0,
            BagInner::One(Some((_, count))) => *count,
            BagInner::Many(bag) => bag.len(),
        }
    }
}

impl<T: Hash + Eq> Bag<T> {
    pub fn insert(&mut self, value: T) {
        match self.inner {
            BagInner::One(None) => self.inner = BagInner::One(Some((value, 1))),
            BagInner::One(Some((ref inner, ref mut count))) if *inner == value => *count += 1,
            BagInner::One(Some(_)) => {
                let (inner, count) = match core::mem::take(self).inner {
                    BagInner::One(Some((value, count))) => (value, count),
                    _ => unreachable!(),
                };
                self.inner = BagInner::One(None);
                let mut bag = HashBag::new();
                bag.insert_many(inner, count);
                bag.insert(value);
                self.inner = BagInner::Many(bag);
            }
            BagInner::Many(ref mut bag) => {
                bag.insert(value);
            }
        }
    }

    pub fn remove(&mut self, value: &T) {
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

    pub fn retain<F: FnMut(&T, usize) -> usize>(&mut self, mut f: F) {
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
    Many(HashBag<T>),
}

pub struct CMultiMap<K, V, S = DefaultHasher, Strat = DefaultStrat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::op::OpWriter<
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::RawDBuf<HashMap<K, Bag<V>, S>>>,
        MapOp<K, V, S>,
    >,
}

pub struct CMultiMapReader<K, V, S = DefaultHasher, Strat = DefaultStrat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::Reader<
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::RawDBuf<HashMap<K, Bag<V>, S>>>,
    >,
}

pub struct CMapReadGuard<'a, K, V, S, Strat = DefaultStrat, T: ?Sized = HashMap<K, Bag<V>, S>>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::ReadGuard<
        'a,
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::RawDBuf<HashMap<K, Bag<V>, S>>>,
        T,
    >,
}

pub enum MapOp<K, V, S> {
    Insert(K, V),
    Clear(K),
    Remove(K, V),
    #[allow(clippy::type_complexity)]
    Arbitrary(SyncWrapper<Box<dyn FnMut(bool, &mut HashMap<K, Bag<V>, S>) + Send>>),
    #[allow(clippy::type_complexity)]
    ArbitraryFor(
        K,
        SyncWrapper<Box<dyn FnMut(bool, K, &mut HashMap<K, Bag<V>, S>) + Send>>,
    ),
    Purge,
}

impl<K: Hash + Eq + Split, V: Split + Hash + Eq, S: BuildHasher>
    dbuf::op_log::Operation<HashMap<K, Bag<V>, S>> for MapOp<K, V, S>
{
    fn apply(&mut self, buffer: &mut HashMap<K, Bag<V>, S>) {
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
            MapOp::ArbitraryFor(ref mut key, f) => f.get_mut()(false, key.split(), buffer),
            MapOp::Purge => buffer.clear(),
        }
    }

    fn apply_last(self, buffer: &mut HashMap<K, Bag<V>, S>) {
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
            MapOp::ArbitraryFor(key, mut f) => f.get_mut()(false, key, buffer),
            MapOp::Purge => buffer.clear(),
        }
    }
}

impl<K, V> CMultiMap<K, V> {
    pub fn new() -> Self {
        Self::from_maps(HashMap::new(), HashMap::new())
    }
}

impl<K, V, S: Default, Strat: Default> Default for CMultiMap<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    fn default() -> Self {
        Self::from_maps(Default::default(), Default::default())
    }
}

impl<K, V, S: Split, Strat> CMultiMap<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible> + Default,
{
    pub fn with_hasher(mut hasher: S) -> Self {
        Self::from_maps(
            HashMap::with_hasher(hasher.split()),
            HashMap::with_hasher(hasher),
        )
    }
}

impl<K, V, S, Strat> CMultiMap<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible> + Default,
{
    pub fn from_maps(front: HashMap<K, Bag<V>, S>, back: HashMap<K, Bag<V>, S>) -> Self {
        Self::from_raw_parts(front, back, Strat::default())
    }
}

impl<K, V, S, Strat> CMultiMap<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn from_raw_parts(
        front: HashMap<K, Bag<V>, S>,
        back: HashMap<K, Bag<V>, S>,
        strategy: Strat,
    ) -> Self {
        Self {
            inner: dbuf::op::OpWriter::from(dbuf::raw::Writer::new(dbuf::ptrs::alloc::Owned::new(
                dbuf::raw::Shared::from_raw_parts(strategy, dbuf::raw::RawDBuf::new(front, back)),
            ))),
        }
    }

    pub fn reader(&self) -> CMultiMapReader<K, V, S, Strat> {
        CMultiMapReader {
            inner: self.inner.reader(),
        }
    }

    pub fn load(&self) -> &HashMap<K, Bag<V>, S> {
        self.inner.split().reader
    }
}

impl<K: Hash + Eq + Split, V: Split + Hash + Eq, S: BuildHasher, Strat> CMultiMap<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn insert(&mut self, key: K, value: V) {
        self.inner.apply(MapOp::Insert(key, value));
    }

    pub fn remove(&mut self, key: K, value: V) {
        self.inner.apply(MapOp::Remove(key, value));
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&Bag<V>>
    where
        Q: ?Sized + Hash + Eq,
        K: Borrow<Q>,
    {
        self.inner.split().reader.get(key)
    }

    pub fn get_one<Q>(&self, key: &Q) -> Option<&V>
    where
        Q: ?Sized + Hash + Eq,
        K: Borrow<Q>,
    {
        self.get(key)?.get_one()
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

    pub fn retain_for(&mut self, key: K, mut f: impl FnMut(bool, &V) -> bool + Send + 'static) {
        self.inner.apply(MapOp::ArbitraryFor(
            key,
            SyncWrapper::new(Box::new(move |is_first, key, map| {
                let bag = map.entry(key);
                if let Entry::Occupied(mut bag) = bag {
                    bag.get_mut().retain(|v, mut count| {
                        #[allow(clippy::mut_range_bound)]
                        for _ in 0..count {
                            count -= usize::from(f(is_first, v))
                        }
                        count
                    });

                    if bag.get().is_empty() {
                        bag.remove();
                    }
                }
            })),
        ))
    }

    pub fn unapplied(&self) -> &[MapOp<K, V, S>] {
        self.inner.unapplied()
    }

    pub fn force_publish(&mut self) {
        self.inner.swap_buffers();
    }

    pub fn publish(&mut self) {
        self.inner.publish()
    }
}

impl<K, V, S, Strat> Clone for CMultiMapReader<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<K, V, S, Strat> CMultiMapReader<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn load(&mut self) -> CMapReadGuard<K, V, S, Strat> {
        CMapReadGuard {
            inner: self.inner.get(),
        }
    }

    pub fn get<Q>(&mut self, key: &Q) -> Option<CMapReadGuard<K, V, S, Strat, Bag<V>>>
    where
        Q: ?Sized + Hash + Eq,
        K: Hash + Eq + Borrow<Q>,
        S: BuildHasher,
    {
        self.load().try_map(|map| map.get(key)).ok()
    }

    pub fn get_one<Q>(&mut self, key: &Q) -> Option<CMapReadGuard<K, V, S, Strat, V>>
    where
        Q: ?Sized + Hash + Eq,
        K: Hash + Eq + Borrow<Q>,
        S: BuildHasher,
    {
        let guard = self.get(key)?;

        CMapReadGuard::try_map(guard, Bag::get_one).ok()
    }
}

impl<K, V, S, Strat, T: ?Sized> Deref for CMapReadGuard<'_, K, V, S, Strat, T>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, K, V, S, Strat, T: ?Sized> CMapReadGuard<'a, K, V, S, Strat, T>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn map<U: ?Sized>(self, f: impl FnOnce(&T) -> &U) -> CMapReadGuard<'a, K, V, S, Strat, U> {
        CMapReadGuard {
            inner: dbuf::raw::ReadGuard::map(self.inner, f),
        }
    }

    pub fn try_map<U: ?Sized>(
        self,
        f: impl FnOnce(&T) -> Option<&U>,
    ) -> Result<CMapReadGuard<'a, K, V, S, Strat, U>, Self> {
        match dbuf::raw::ReadGuard::try_map(self.inner, f) {
            Ok(inner) => Ok(CMapReadGuard { inner }),
            Err(inner) => Err(CMapReadGuard { inner }),
        }
    }
}

impl<K, V, S, Strat, T: ?Sized + core::fmt::Debug> core::fmt::Debug
    for CMapReadGuard<'_, K, V, S, Strat, T>
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
    Many(hashbag::Iter<'a, T>),
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
