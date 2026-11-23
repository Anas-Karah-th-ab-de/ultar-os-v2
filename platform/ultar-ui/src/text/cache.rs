//! A two-generation cache with a cost limit (the approach of the old shell's text cache:
//! when the young generation is full it becomes the old one, and whatever is still used
//! migrates back instead of being rebuilt).

use std::collections::HashMap;
use std::hash::Hash;

/// Counters of a cache, for tests and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    /// Entries currently held (both generations).
    pub entries: usize,
    /// Cost currently held (bytes for glyphs, entries for layouts).
    pub cost: usize,
    /// Lookups answered from the cache.
    pub hits: u64,
    /// Lookups that had to build the value.
    pub misses: u64,
}

pub(crate) struct TwoGen<K, V> {
    young: HashMap<K, (V, usize)>,
    old: HashMap<K, (V, usize)>,
    young_cost: usize,
    old_cost: usize,
    limit: usize,
    hits: u64,
    misses: u64,
}

impl<K: Hash + Eq + Clone, V: Clone> TwoGen<K, V> {
    /// `limit` is the cost one generation may hold; total held cost never exceeds
    /// `2 * limit`. A value that alone costs more than `limit` is returned but not kept.
    pub(crate) fn new(limit: usize) -> Self {
        TwoGen {
            young: HashMap::new(),
            old: HashMap::new(),
            young_cost: 0,
            old_cost: 0,
            limit: limit.max(1),
            hits: 0,
            misses: 0,
        }
    }

    /// Returns the cached value or builds it; `make` returns the value and its cost.
    pub(crate) fn get_or_insert_with(&mut self, key: &K, make: impl FnOnce() -> (V, usize)) -> V {
        if let Some(entry) = self.young.get(key) {
            self.hits += 1;
            return entry.0.clone();
        }
        let (value, cost) = match self.old.remove(key) {
            Some(entry) => {
                self.hits += 1;
                self.old_cost = self.old_cost.saturating_sub(entry.1);
                entry
            }
            None => {
                self.misses += 1;
                make()
            }
        };
        if cost > self.limit {
            // Larger than a whole generation (a glyph at an absurd size): never held.
            return value;
        }
        if self.young_cost + cost > self.limit && !self.young.is_empty() {
            self.old = std::mem::take(&mut self.young);
            self.old_cost = self.young_cost;
            self.young_cost = 0;
        }
        self.young_cost += cost;
        self.young.insert(key.clone(), (value.clone(), cost));
        value
    }

    /// Replaces the value of `key` (used when a hash key turned out to be a collision).
    pub(crate) fn replace(&mut self, key: &K, value: V, cost: usize) {
        if cost > self.limit {
            if let Some(previous) = self.young.remove(key) {
                self.young_cost = self.young_cost.saturating_sub(previous.1);
            }
            return;
        }
        if let Some(previous) = self.young.insert(key.clone(), (value, cost)) {
            self.young_cost = self.young_cost.saturating_sub(previous.1);
        }
        self.young_cost += cost;
    }

    pub(crate) fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.young.len() + self.old.len(),
            cost: self.young_cost + self.old_cost,
            hits: self.hits,
            misses: self.misses,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.young.clear();
        self.old.clear();
        self.young_cost = 0;
        self.old_cost = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_misses_and_limit() {
        let mut cache: TwoGen<u32, u32> = TwoGen::new(10);
        for round in 0..3 {
            for key in 0..100u32 {
                assert_eq!(cache.get_or_insert_with(&key, || (key * 2, 1)), key * 2);
                let stats = cache.stats();
                assert!(stats.cost <= 20, "round {round}: {stats:?}");
                assert!(stats.entries <= 20);
            }
        }
        assert_eq!(cache.stats().misses, 300);
        assert_eq!(cache.get_or_insert_with(&99, || (0, 1)), 198);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn used_entries_survive_a_generation_change() {
        let mut cache: TwoGen<u32, u32> = TwoGen::new(4);
        cache.get_or_insert_with(&1, || (10, 1));
        for key in 100..104 {
            cache.get_or_insert_with(&key, || (0, 1));
        }
        // Key 1 is in the old generation now and comes back without being rebuilt.
        assert_eq!(cache.get_or_insert_with(&1, || (99, 1)), 10);
        cache.replace(&1, 11, 1);
        assert_eq!(cache.get_or_insert_with(&1, || (99, 1)), 11);
        cache.clear();
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.stats().cost, 0);
    }

    #[test]
    fn oversized_entry_is_still_returned() {
        let mut cache: TwoGen<u32, u32> = TwoGen::new(4);
        assert_eq!(cache.get_or_insert_with(&7, || (70, 1000)), 70);
        assert_eq!(
            cache.stats().cost,
            0,
            "an entry above the limit is not kept"
        );
        cache.get_or_insert_with(&8, || (80, 1));
        assert_eq!((cache.stats().entries, cache.stats().cost), (1, 1));
        cache.replace(&8, 81, 1000);
        assert_eq!((cache.stats().entries, cache.stats().cost), (0, 0));
        assert_eq!(cache.get_or_insert_with(&8, || (82, 1)), 82);
    }
}
