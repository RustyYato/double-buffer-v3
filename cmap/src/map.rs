use super::{DefaultHasher, DefaultStrat};
use std::{
    borrow::Borrow,
    collections::HashMap,
    convert::Infallible,
    hash::{BuildHasher, Hash},
    ops::Deref,
};

use dbuf::interface::Strategy;
use sync_wrapper::SyncWrapper;

use crate::split::Split;

pub struct CMap<K, V, S = DefaultHasher, Strat = DefaultStrat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::op::OpWriter<
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::RawDBuf<HashMap<K, V, S>>>,
        MapOp<K, V, S>,
    >,
}

pub struct CMapReader<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner:
        dbuf::raw::Reader<dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::RawDBuf<HashMap<K, V, S>>>>,
}

pub struct CMapReadGuard<'a, K, V, S = DefaultHasher, Strat = DefaultStrat, T = HashMap<K, V, S>>
where
    T: ?Sized,
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::ReadGuard<
        'a,
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::RawDBuf<HashMap<K, V, S>>>,
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

impl<K, V, S> dbuf::op_log::Operation<HashMap<K, V, S>> for MapOp<K, V, S>
where
    K: Hash + Eq + Split,
    V: Split,
    S: BuildHasher,
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

impl<K, V, S, Strat> Default for CMap<K, V, S, Strat>
where
    S: Default,
    Strat: Strategy<ValidationError = Infallible> + Default,
{
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

impl<K, V, S, Strat> CMap<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible> + Default,
{
    pub fn from_maps(front: HashMap<K, V, S>, back: HashMap<K, V, S>) -> Self {
        Self::from_raw_parts(front, back, Strat::default())
    }
}

impl<K, V, S, Strat> CMap<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn from_raw_parts(
        front: HashMap<K, V, S>,
        back: HashMap<K, V, S>,
        strategy: Strat,
    ) -> Self {
        Self {
            inner: dbuf::op::OpWriter::from(dbuf::raw::Writer::new(dbuf::ptrs::alloc::Owned::new(
                dbuf::raw::Shared::from_raw_parts(strategy, dbuf::raw::RawDBuf::new(front, back)),
            ))),
        }
    }

    pub fn reader(&self) -> CMapReader<K, V, S, Strat> {
        CMapReader {
            inner: self.inner.reader(),
        }
    }

    pub fn load(&self) -> &HashMap<K, V, S> {
        self.inner.split().reader
    }
}

impl<K, V, S, Strat> CMap<K, V, S, Strat>
where
    K: Hash + Eq + Split,
    V: Split,
    S: BuildHasher,
    Strat: Strategy<ValidationError = Infallible>,
{
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

impl<K, V, S, Strat> Clone for CMapReader<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<K, V, S, Strat> CMapReader<K, V, S, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn load(&mut self) -> CMapReadGuard<K, V, S, Strat> {
        CMapReadGuard {
            inner: self.inner.get(),
        }
    }

    pub fn get<Q>(&mut self, key: &Q) -> Option<CMapReadGuard<K, V, S, Strat, V>>
    where
        Q: ?Sized + Hash + Eq,
        K: Hash + Eq + Borrow<Q>,
        S: BuildHasher,
    {
        self.load().try_map(|map| map.get(key)).ok()
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
