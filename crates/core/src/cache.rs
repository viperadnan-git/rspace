use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

/// Outcome of a [`QueryCache::lookup`] under the stale-while-revalidate policy.
pub enum Lookup<'a, V> {
    /// Cached and within the stale window — serve it, no fetch needed.
    Fresh(&'a V),
    /// Cached but past the stale window — serve it, refetch in the background.
    Stale(&'a V),
    /// Nothing cached — fetch.
    Miss,
}

struct Stamped<V> {
    value: V,
    fetched_at: Instant,
}

/// A stale-while-revalidate cache: stores values with their fetch time and
/// classifies a key as fresh / stale / miss against `stale_after`. Owns only
/// storage and policy; fetching and applying results is the caller's job.
pub struct QueryCache<K, V> {
    stale_after: Duration,
    entries: HashMap<K, Stamped<V>>,
}

impl<K: Eq + Hash, V> QueryCache<K, V> {
    pub fn new(stale_after: Duration) -> Self {
        Self { stale_after, entries: HashMap::new() }
    }

    /// Classify `key` as fresh, stale, or a miss.
    pub fn lookup(&self, key: &K) -> Lookup<'_, V> {
        match self.entries.get(key) {
            None => Lookup::Miss,
            Some(s) if s.fetched_at.elapsed() < self.stale_after => Lookup::Fresh(&s.value),
            Some(s) => Lookup::Stale(&s.value),
        }
    }

    /// Store a freshly fetched value, stamping it as fetched now.
    pub fn insert(&mut self, key: K, value: V) {
        self.entries.insert(key, Stamped { value, fetched_at: Instant::now() });
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|s| &s.value)
    }

    /// Mutable access without resetting freshness (e.g. to re-sort in place).
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries.get_mut(key).map(|s| &mut s.value)
    }

    pub fn set_stale_after(&mut self, stale_after: Duration) {
        self.stale_after = stale_after;
    }

    pub fn invalidate(&mut self, key: &K) {
        self.entries.remove(key);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miss_when_absent() {
        let cache: QueryCache<u32, i32> = QueryCache::new(Duration::from_secs(60));
        assert!(matches!(cache.lookup(&1), Lookup::Miss));
    }

    #[test]
    fn fresh_within_window() {
        let mut cache = QueryCache::new(Duration::from_secs(3600));
        cache.insert(1, vec!["a"]);
        assert!(matches!(cache.lookup(&1), Lookup::Fresh(_)));
    }

    #[test]
    fn stale_past_window() {
        // Zero window: anything already inserted is immediately stale.
        let mut cache = QueryCache::new(Duration::ZERO);
        cache.insert(1, 10);
        assert!(matches!(cache.lookup(&1), Lookup::Stale(_)));
    }

    #[test]
    fn invalidate_then_miss() {
        let mut cache = QueryCache::new(Duration::from_secs(60));
        cache.insert(1, 1);
        cache.invalidate(&1);
        assert!(matches!(cache.lookup(&1), Lookup::Miss));
    }
}
