use super::DefaultStrat;
use std::{borrow::Borrow, collections::BTreeMap, convert::Infallible, ops::Deref};

use dbuf::interface::Strategy;
use sync_wrapper::SyncWrapper;

use crate::split::Split;

pub struct CBTreeMap<K, V, Strat = DefaultStrat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::op::OpWriter<
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::SizedRawDoubleBuffer<BTreeMap<K, V>>>,
        MapOp<K, V>,
    >,
}

pub struct CBTreeMapReader<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::Reader<
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::SizedRawDoubleBuffer<BTreeMap<K, V>>>,
    >,
}

pub struct CBTreeMapReadGuard<'a, K, V, Strat = DefaultStrat, T = BTreeMap<K, V>>
where
    T: ?Sized,
    Strat: Strategy<ValidationError = Infallible>,
{
    #[allow(clippy::type_complexity)]
    inner: dbuf::raw::ReadGuard<
        'a,
        dbuf::ptrs::alloc::OwnedPtr<Strat, dbuf::raw::SizedRawDoubleBuffer<BTreeMap<K, V>>>,
        T,
    >,
}

pub enum MapOp<K, V> {
    Insert(K, V),
    Remove(K),
    #[allow(clippy::type_complexity)]
    Arbitrary(SyncWrapper<Box<dyn FnMut(bool, &mut BTreeMap<K, V>) + Send>>),
    Clear,
}

impl<K, V> dbuf::op_log::Operation<BTreeMap<K, V>> for MapOp<K, V>
where
    K: Ord + Split,
    V: Split,
{
    fn apply(&mut self, buffer: &mut BTreeMap<K, V>) {
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

    fn apply_last(self, buffer: &mut BTreeMap<K, V>) {
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

impl<K, V> CBTreeMap<K, V> {
    pub fn new() -> Self {
        Self::from_maps(BTreeMap::new(), BTreeMap::new())
    }
}

impl<K, V, Strat> Default for CBTreeMap<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible> + Default,
{
    fn default() -> Self {
        Self::from_maps(Default::default(), Default::default())
    }
}

impl<K, V, Strat> CBTreeMap<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible> + Default,
{
    pub fn from_maps(front: BTreeMap<K, V>, back: BTreeMap<K, V>) -> Self {
        Self::from_raw_parts(front, back, Strat::default())
    }
}

impl<K, V, Strat> CBTreeMap<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn from_raw_parts(front: BTreeMap<K, V>, back: BTreeMap<K, V>, strategy: Strat) -> Self {
        Self {
            inner: dbuf::op::OpWriter::from(dbuf::raw::Writer::new(dbuf::ptrs::alloc::Owned::new(
                dbuf::raw::Shared::from_raw_parts(
                    strategy,
                    dbuf::raw::SizedRawDoubleBuffer::new(front, back),
                ),
            ))),
        }
    }

    pub fn reader(&self) -> CBTreeMapReader<K, V, Strat> {
        CBTreeMapReader {
            inner: self.inner.reader(),
        }
    }

    pub fn load(&self) -> &BTreeMap<K, V> {
        self.inner.split().reader
    }
}

impl<K, V, Strat> CBTreeMap<K, V, Strat>
where
    K: Ord + Split,
    V: Split,
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
        Q: ?Sized + Ord,
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

impl<K, V, Strat> Clone for CBTreeMapReader<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<K, V, Strat> CBTreeMapReader<K, V, Strat>
where
    Strat: Strategy<ValidationError = Infallible>,
{
    pub fn load(&mut self) -> CBTreeMapReadGuard<K, V, Strat> {
        CBTreeMapReadGuard {
            inner: self.inner.get(),
        }
    }

    pub fn get<Q>(&mut self, key: &Q) -> Option<CBTreeMapReadGuard<K, V, Strat, V>>
    where
        Q: ?Sized + Ord,
        K: Ord + Borrow<Q>,
    {
        self.load().try_map(|map| map.get(key)).ok()
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
