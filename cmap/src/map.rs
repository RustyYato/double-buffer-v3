use std::{
    borrow::Borrow,
    collections::{hash_map::RandomState, HashMap},
    hash::{BuildHasher, Hash},
    ops::Deref,
};

use sync_wrapper::SyncWrapper;

use crate::split::Split;

type Waiter = dbuf::wait::DefaultWait;

pub struct CMap<K, V, S = RandomState> {
    #[allow(clippy::type_complexity)]
    inner: dbuf::op::OpWriter<
        dbuf::ptrs::alloc::OwnedPtr<
            dbuf::strategy::HazardStrategy<Waiter>,
            dbuf::raw::SizedRawDoubleBuffer<HashMap<K, V, S>>,
        >,
        MapOp<K, V, S>,
    >,
}

pub struct CMapReader<K, V, S = RandomState> {
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::Reader<
        dbuf::ptrs::alloc::OwnedPtr<
            dbuf::strategy::HazardStrategy<Waiter>,
            dbuf::raw::SizedRawDoubleBuffer<HashMap<K, V, S>>,
        >,
    >,
}

pub struct CMapReadGuard<'a, K, V, S, T: ?Sized = HashMap<K, V, S>> {
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::ReadGuard<
        'a,
        dbuf::ptrs::alloc::OwnedPtr<
            dbuf::strategy::HazardStrategy<Waiter>,
            dbuf::raw::SizedRawDoubleBuffer<HashMap<K, V, S>>,
        >,
        T,
    >,
}

pub enum MapOp<K, V, S> {
    Insert(K, V),
    Remove(K),
    #[allow(clippy::type_complexity)]
    Arbitrary(SyncWrapper<Box<dyn FnMut(bool, &mut HashMap<K, V, S>) + Send>>),
    Clear,
}

impl<K: Hash + Eq + Split, V: Split, S: BuildHasher> dbuf::op_log::Operation<HashMap<K, V, S>>
    for MapOp<K, V, S>
{
    fn apply(&mut self, buffer: &mut HashMap<K, V, S>) {
        match self {
            MapOp::Insert(key, value) => {
                buffer.insert(key.split(), value.split());
            }
            MapOp::Remove(key) => {
                buffer.remove(key);
            }
            MapOp::Arbitrary(f) => f.get_mut()(false, buffer),
            MapOp::Clear => buffer.clear(),
        }
    }

    fn apply_last(self, buffer: &mut HashMap<K, V, S>) {
        match self {
            MapOp::Insert(key, value) => {
                buffer.insert(key, value);
            }
            MapOp::Remove(ref key) => {
                buffer.remove(key);
            }
            MapOp::Arbitrary(f) => f.into_inner()(true, buffer),
            MapOp::Clear => buffer.clear(),
        }
    }
}

impl<K, V> CMap<K, V> {
    pub fn new() -> Self {
        Self::from_maps(HashMap::new(), HashMap::new())
    }
}

impl<K, V, S: Default> Default for CMap<K, V, S> {
    fn default() -> Self {
        Self::from_maps(Default::default(), Default::default())
    }
}

impl<K, V, S: Split> CMap<K, V, S> {
    pub fn with_hasher(mut hasher: S) -> Self {
        Self::from_maps(
            HashMap::with_hasher(hasher.split()),
            HashMap::with_hasher(hasher),
        )
    }
}

impl<K, V, S> CMap<K, V, S> {
    pub fn reader(&self) -> CMapReader<K, V, S> {
        CMapReader {
            inner: self.inner.reader(),
        }
    }

    pub fn from_maps(front: HashMap<K, V, S>, back: HashMap<K, V, S>) -> Self {
        Self {
            inner: dbuf::op::OpWriter::from(dbuf::raw::Writer::new(
                dbuf::ptrs::alloc::Owned::from_buffers(front, back),
            )),
        }
    }

    pub fn load(&self) -> &HashMap<K, V, S> {
        self.inner.split().reader
    }
}

impl<K: Hash + Eq + Split, V: Split, S: BuildHasher> CMap<K, V, S> {
    pub fn insert(&mut self, key: K, value: V) {
        self.inner.apply(MapOp::Insert(key, value));
    }

    pub fn remove(&mut self, key: K) {
        self.inner.apply(MapOp::Remove(key));
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        Q: ?Sized + Hash + Eq,
        K: Borrow<Q>,
    {
        self.inner.split().reader.get(key)
    }

    pub fn clear(&mut self) {
        self.inner.apply(MapOp::Clear)
    }

    pub fn retain(&mut self, mut f: impl FnMut(bool, &K, &mut V) -> bool + Send + 'static) {
        self.inner.apply(MapOp::Arbitrary(SyncWrapper::new(Box::new(
            move |is_first, map| map.retain(|k, v| f(is_first, k, v)),
        ))))
    }

    pub fn flush(&mut self) {
        self.inner.swap_buffers();
    }
}

impl<K, V, S> Clone for CMapReader<K, V, S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.copy_tag(),
        }
    }
}

impl<K, V, S> CMapReader<K, V, S> {
    pub fn load(&mut self) -> CMapReadGuard<K, V, S> {
        CMapReadGuard {
            inner: self.inner.get(),
        }
    }

    pub fn get<Q>(&mut self, key: &Q) -> Option<CMapReadGuard<K, V, S, V>>
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
