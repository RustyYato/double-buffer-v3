use std::{
    borrow::Borrow,
    collections::{hash_map::RandomState, HashMap},
    hash::{BuildHasher, Hash},
    ops::Deref,
};

use hashbag::HashBag;
use sync_wrapper::SyncWrapper;

use crate::split::Split;

type Strat = dbuf::strategy::HazardStrategy<dbuf::wait::DefaultWait>;

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

impl<T: Hash + Eq> Bag<T> {
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
    Many(HashBag<T>),
}

pub struct CMultiMap<K, V, S = RandomState> {
    #[allow(clippy::type_complexity)]
    inner: dbuf::op::OpWriter<
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::SizedRawDoubleBuffer<HashMap<K, Bag<V>, S>>>,
        MapOp<K, V, S>,
    >,
}

pub struct CMultiMapReader<K, V, S = RandomState> {
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::Reader<
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::SizedRawDoubleBuffer<HashMap<K, Bag<V>, S>>>,
    >,
}

pub struct CMapReadGuard<'a, K, V, S, T: ?Sized = HashMap<K, Bag<V>, S>> {
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::ReadGuard<
        'a,
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::SizedRawDoubleBuffer<HashMap<K, Bag<V>, S>>>,
        T,
    >,
}

pub enum MapOp<K, V, S> {
    Insert(K, V),
    Clear(K),
    Remove(K, V),
    #[allow(clippy::type_complexity)]
    Arbitrary(SyncWrapper<Box<dyn FnMut(bool, &mut HashMap<K, Bag<V>, S>) + Send>>),
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
            MapOp::Purge => buffer.clear(),
        }
    }
}

impl<K, V> CMultiMap<K, V> {
    pub fn new() -> Self {
        Self::from_maps(HashMap::new(), HashMap::new())
    }
}

impl<K, V, S: Default> Default for CMultiMap<K, V, S> {
    fn default() -> Self {
        Self::from_maps(Default::default(), Default::default())
    }
}

impl<K, V, S: Split> CMultiMap<K, V, S> {
    pub fn with_hasher(mut hasher: S) -> Self {
        Self::from_maps(
            HashMap::with_hasher(hasher.split()),
            HashMap::with_hasher(hasher),
        )
    }
}

impl<K, V, S> CMultiMap<K, V, S> {
    pub fn reader(&self) -> CMultiMapReader<K, V, S> {
        CMultiMapReader {
            inner: self.inner.reader(),
        }
    }

    pub fn from_maps(front: HashMap<K, Bag<V>, S>, back: HashMap<K, Bag<V>, S>) -> Self {
        Self {
            inner: dbuf::op::OpWriter::from(dbuf::raw::Writer::new(
                dbuf::ptrs::alloc::Owned::from_buffers(front, back),
            )),
        }
    }

    pub fn load(&self) -> &HashMap<K, Bag<V>, S> {
        self.inner.split().reader
    }
}

impl<K: Hash + Eq + Split, V: Split + Hash + Eq, S: BuildHasher> CMultiMap<K, V, S> {
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

    pub fn flush(&mut self) {
        self.inner.swap_buffers();
    }
}

impl<K, V, S> Clone for CMultiMapReader<K, V, S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<K, V, S> CMultiMapReader<K, V, S> {
    pub fn load(&mut self) -> CMapReadGuard<K, V, S> {
        CMapReadGuard {
            inner: self.inner.get(),
        }
    }

    pub fn get<Q>(&mut self, key: &Q) -> Option<CMapReadGuard<K, V, S, Bag<V>>>
    where
        Q: ?Sized + Hash + Eq,
        K: Hash + Eq + Borrow<Q>,
        S: BuildHasher,
    {
        self.load().try_map(|map| map.get(key)).ok()
    }
}

impl<K, V, S, T: ?Sized> Deref for CMapReadGuard<'_, K, V, S, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, K, V, S, T: ?Sized> CMapReadGuard<'a, K, V, S, T> {
    pub fn map<U: ?Sized>(self, f: impl FnOnce(&T) -> &U) -> CMapReadGuard<'a, K, V, S, U> {
        CMapReadGuard {
            inner: dbuf::raw::ReadGuard::map(self.inner, f),
        }
    }

    pub fn try_map<U: ?Sized>(
        self,
        f: impl FnOnce(&T) -> Option<&U>,
    ) -> Result<CMapReadGuard<'a, K, V, S, U>, Self> {
        match dbuf::raw::ReadGuard::try_map(self.inner, f) {
            Ok(inner) => Ok(CMapReadGuard { inner }),
            Err(inner) => Err(CMapReadGuard { inner }),
        }
    }
}

impl<K, V, S, T: ?Sized + core::fmt::Debug> core::fmt::Debug for CMapReadGuard<'_, K, V, S, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        T::fmt(self, f)
    }
}
