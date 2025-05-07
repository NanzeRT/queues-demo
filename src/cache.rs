use std::{
    borrow::Borrow,
    fmt::Debug,
    future::Future,
    hash::Hash,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use dashmap::DashMap;
use dlv_list::{Index, VecList};
use tokio::task;

use crate::utils::Timed;

#[derive(Debug)]
pub struct ImportantExpires<K> {
    pub key: K,
    pub usages: u64,
}

#[derive(Debug)]
pub enum CacheError {
    KeyExists,
    KeyNotFound,
    UsageUnderflow,
    NoncriticalError,
}

pub trait DataGetter {
    type Key: Borrow<Self::BorrowedKey> + for<'a> From<&'a Self::BorrowedKey>;
    type BorrowedKey: ?Sized;
    type Value;
    fn get(&self, key: &Self::BorrowedKey) -> impl Future<Output = Self::Value>;
}

#[derive(Debug, Default)]
pub struct Cache<G: DataGetter, const IDLE_EXPIRE_MILLIS: u128, const USED_EXPIRE_MILLIS: u128>
where
    G::Key: Hash + Eq + Clone,
{
    cached: MapWithExpires<G::Key, G::Value, IDLE_EXPIRE_MILLIS, USED_EXPIRE_MILLIS>,
    getter: G,
}

impl<G, const FE: u128, const SE: u128> Cache<G, FE, SE>
where
    G: DataGetter,
    G::Key: Hash + Eq + Clone + Debug,
    G::Value: Clone + Default,
    G::BorrowedKey: Hash + Eq,
{
    pub async fn get(&self, key: &G::BorrowedKey) -> G::Value {
        match self.cached.get(key) {
            Some(value) => value,
            None => self.fetch_and_set(key).await,
        }
    }

    pub fn set(&self, key: G::Key, value: G::Value) -> Result<(), CacheError> {
        self.cached.set(key, value)
    }

    pub async fn add_usage(&self, key: &G::BorrowedKey) -> Result<(), CacheError> {
        self.cached.add_usage(key).await
    }

    pub async fn remove_usage(&self, key: &G::BorrowedKey) -> Result<(), CacheError> {
        self.cached.remove_usage(key).await
    }

    #[must_use]
    pub fn evict_expired(&self) -> Vec<ImportantExpires<G::Key>> {
        self.cached.evict_expired()
    }

    async fn fetch_and_set(&self, key: &G::BorrowedKey) -> G::Value {
        let data: G::Value = self.getter.get(key).await;
        self.cached.set(key.into(), data.clone()).ok();
        data
    }
}

#[derive(Debug)]
struct MapWithExpires<K, V, const FAST_EXPIRE_MILLIS: u128, const SLOW_EXPIRE_MILLIS: u128>
where
    K: Hash + Eq + Clone,
{
    // NOTE: lock in order of definition
    idle: Mutex<VecList<Timed<K>>>,
    used: Mutex<VecList<Timed<K>>>,
    data: DashMap<K, MapEntry<K, V>>,
}

#[derive(Debug)]
struct MapEntry<K, V> {
    value: V,
    index: Index<Timed<K>>,
    counter: AtomicU64,
}

impl<K, V, const FE: u128, const SE: u128> Default for MapWithExpires<K, V, FE, SE>
where
    K: Hash + Eq + Clone,
{
    fn default() -> Self {
        Self {
            idle: Mutex::new(VecList::new()),
            used: Mutex::new(VecList::new()),
            data: DashMap::new(),
        }
    }
}

impl<K, V, const FAST_EXPIRE_MILLIS: u128, const SLOW_EXPIRE_MILLIS: u128>
    MapWithExpires<K, V, FAST_EXPIRE_MILLIS, SLOW_EXPIRE_MILLIS>
where
    K: Hash + Eq + Clone,
    V: Clone,
{
    pub fn get<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q> + for<'a> From<&'a Q>,
        Q: Hash + Eq + ?Sized,
    {
        let (value, counter) = {
            let kv_pair = self.data.get(key)?;
            let MapEntry { value, counter, .. } = kv_pair.value();
            (value.clone(), counter.load(Ordering::Relaxed))
        };
        if counter == 0 {
            self.renew_idle(key);
        }
        Some(value)
    }

    pub fn set(&self, key: K, value: V) -> Result<(), CacheError> {
        let index = self
            .idle
            .lock()
            .expect("Mutex poisoned")
            .push_back(Timed::new(key.clone()));
        let counter = AtomicU64::new(0);
        match self.data.entry(key) {
            dashmap::Entry::Vacant(entry) => {
                entry.insert(MapEntry {
                    value,
                    index,
                    counter,
                });
                Ok(())
            }
            dashmap::Entry::Occupied(_) => {
                self.idle.lock().expect("Mutex poisoned").remove(index);
                Err(CacheError::KeyExists)
            }
        }
    }

    pub async fn add_usage<Q>(&self, key: &Q) -> Result<(), CacheError>
    where
        K: Borrow<Q> + for<'a> From<&'a Q>,
        Q: Hash + Eq + ?Sized,
    {
        let kv_pair = self.data.get(key).ok_or(CacheError::KeyNotFound)?;
        let MapEntry { index, counter, .. } = kv_pair.value();
        let index = *index;
        let prev = counter.fetch_add(1, Ordering::Relaxed);
        drop(kv_pair);
        if prev == 0 {
            for _ in 0..10 {
                {
                    let mut idle = self.idle.lock().expect("Mutex poisoned");
                    let mut used = self.used.lock().expect("Mutex poisoned");
                    if let Some(Timed { value, .. }) = idle.remove(index) {
                        self.data
                            .get_mut(key)
                            .expect("Cannot happen since removing requires is locked")
                            .index = used.push_back(Timed::new(value));
                        return Ok(());
                    }
                }
                task::yield_now().await;
            }
            return Err(CacheError::NoncriticalError);
        }
        Ok(())
    }

    pub async fn remove_usage<Q>(&self, key: &Q) -> Result<(), CacheError>
    where
        K: Borrow<Q> + for<'a> From<&'a Q>,
        Q: Hash + Eq + ?Sized,
    {
        let kv_pair = self.data.get(key).ok_or(CacheError::KeyNotFound)?;
        let MapEntry { index, counter, .. } = kv_pair.value();
        let index = *index;
        let prev = counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| {
                if x > 0 {
                    Some(x - 1)
                } else {
                    None
                }
            })
            .map_err(|_| CacheError::UsageUnderflow)?;
        drop(kv_pair);
        if prev == 1 {
            for _ in 0..10 {
                {
                    let mut idle = self.idle.lock().expect("Mutex poisoned");
                    let mut used = self.used.lock().expect("Mutex poisoned");
                    if let Some(Timed { value, .. }) = used.remove(index) {
                        self.data
                            .get_mut(key)
                            .expect("Cannot happen since removing requires locking")
                            .index = idle.push_back(Timed::new(value));
                        return Ok(());
                    }
                }
                task::yield_now().await;
            }
            return Err(CacheError::NoncriticalError);
        }
        Ok(())
    }

    #[must_use]
    pub fn evict_expired(&self) -> Vec<ImportantExpires<K>> {
        let mut idle = self.idle.lock().expect("Mutex poisoned");
        while let Some(task) = idle.front() {
            if task.timestamp.elapsed().as_millis() > FAST_EXPIRE_MILLIS {
                let Timed { value: key, .. } = idle.pop_front().expect("Unreachable");
                self.data.remove(key.borrow()).expect("Invariant violated");
            } else {
                break;
            }
        }
        let mut expires = vec![];
        let mut used = self.used.lock().expect("Mutex poisoned");
        while let Some(task) = used.front() {
            if task.timestamp.elapsed().as_millis() > SLOW_EXPIRE_MILLIS {
                let Timed { value: key, .. } = used.pop_front().expect("Unreachable");
                let (key, value) = self.data.remove(key.borrow()).expect("Invariant violated");
                expires.push(ImportantExpires {
                    key,
                    usages: value.counter.load(Ordering::Relaxed),
                });
            } else {
                break;
            }
        }
        expires
    }

    fn renew_idle<Q>(&self, key: &Q)
    where
        K: Borrow<Q> + for<'a> From<&'a Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mut idle = self.idle.lock().expect("Mutex poisoned");
        let Some(mut element) = self.data.get_mut(key) else {
            // TODO: better logging
            eprintln!("Sometimes unlucky");
            return;
        };
        let Some(Timed { value, .. }) = idle.remove(element.index) else {
            assert!(*element.counter.get_mut() != 0, "Invariant violated");
            return;
        };
        element.index = idle.push_back(Timed::new(value));
    }
}
