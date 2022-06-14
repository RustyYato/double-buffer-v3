#![forbid(unsafe_code)]

use std::{
    borrow::Borrow,
    collections::{hash_map::RandomState, HashMap},
    hash::{BuildHasher, Hash},
    ops::Deref,
};

type Waiter = dbuf::wait::DefaultWait;

pub struct CMap<K, V, S = RandomState> {
    #[allow(clippy::type_complexity)]
    inner: dbuf::op::OpWriter<
        dbuf::ptrs::alloc::OwnedStrong<
            dbuf::strategy::HazardStrategy<Waiter>,
            dbuf::raw::SizedRawDoubleBuffer<HashMap<K, V, S>>,
        >,
        MapOp<K, V>,
    >,
}

pub struct CMapReader<K, V, S = RandomState> {
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::Reader<
        dbuf::ptrs::alloc::OwnedWeak<
            dbuf::strategy::HazardStrategy<Waiter>,
            dbuf::raw::SizedRawDoubleBuffer<HashMap<K, V, S>>,
        >,
    >,
}

pub struct CMapReadGuard<'a, K, V, S, T: ?Sized = HashMap<K, V, S>> {
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::ReadGuard<
        'a,
        dbuf::ptrs::alloc::OwnedStrong<
            dbuf::strategy::HazardStrategy<Waiter>,
            dbuf::raw::SizedRawDoubleBuffer<HashMap<K, V, S>>,
        >,
        T,
    >,
}

pub enum MapOp<K, V> {
    Insert(K, V),
    Remove(K),
}

impl<K: Hash + Eq + Clone, V: Clone, S: BuildHasher> dbuf::op_log::Operation<HashMap<K, V, S>>
    for MapOp<K, V>
{
    fn apply(&mut self, buffer: &mut HashMap<K, V, S>) {
        match self {
            MapOp::Insert(key, value) => {
                buffer.insert(key.clone(), value.clone());
            }
            MapOp::Remove(key) => {
                buffer.remove(key);
            }
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

impl<K, V, S: Clone> CMap<K, V, S> {
    pub fn with_hasher(hasher: S) -> Self {
        Self::from_maps(
            HashMap::with_hasher(hasher.clone()),
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
            inner: dbuf::op::OpWriter::from(dbuf::raw::Writer::new(dbuf::ptrs::alloc::Owned::new(
                dbuf::raw::Shared::new(
                    dbuf::strategy::HazardStrategy::default(),
                    dbuf::raw::SizedRawDoubleBuffer::new(front, back),
                ),
            ))),
        }
    }
}

impl<K: Hash + Eq + Clone, V: Clone, S: BuildHasher> CMap<K, V, S> {
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
    pub fn load(&mut self) -> Result<CMapReadGuard<K, V, S>, dbuf::ptrs::alloc::UpgradeError> {
        Ok(CMapReadGuard {
            inner: self.inner.try_get()?,
        })
    }
}

impl<K, V, S, T: ?Sized> Deref for CMapReadGuard<'_, K, V, S, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
