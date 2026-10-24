//! Bounded in-memory cache for values derived from a file fingerprint.

use crate::model::Fingerprint;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

pub(crate) struct FingerprintCache<K, V> {
    entries: HashMap<K, (Fingerprint, V)>,
    insertion_order: VecDeque<K>,
    limit: usize,
}

impl<K: Eq + Hash, V: Clone> FingerprintCache<K, V> {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            insertion_order: VecDeque::new(),
            limit,
        }
    }

    pub(crate) fn get(&self, key: &K, fingerprint: Fingerprint) -> Option<V> {
        self.entries
            .get(key)
            .and_then(|(stored, value)| (*stored == fingerprint).then(|| value.clone()))
    }

    pub(crate) fn insert(&mut self, key: K, fingerprint: Fingerprint, value: V)
    where
        K: Clone,
    {
        if self.limit == 0 {
            return;
        }
        if let Some(entry) = self.entries.get_mut(&key) {
            *entry = (fingerprint, value);
            return;
        }
        // Replacements and lookups preserve the original insertion order.
        if self.entries.len() >= self.limit
            && let Some(oldest) = self.insertion_order.pop_front()
        {
            self.entries.remove(&oldest);
        }
        self.insertion_order.push_back(key.clone());
        self.entries.insert(key, (fingerprint, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint(inode: u64) -> Fingerprint {
        Fingerprint {
            dev: 1,
            inode,
            size: 1,
            mtime: (0, 0),
        }
    }

    #[test]
    fn hit_miss_replacement_and_capacity() {
        let mut cache = FingerprintCache::new(2);
        cache.insert("first", fingerprint(1), 10);
        assert_eq!(cache.get(&"first", fingerprint(1)), Some(10));
        assert_eq!(cache.get(&"first", fingerprint(2)), None);
        cache.insert("first", fingerprint(2), 20);
        assert_eq!(cache.get(&"first", fingerprint(2)), Some(20));
        cache.insert("second", fingerprint(3), 30);
        cache.insert("third", fingerprint(4), 40);
        assert_eq!(cache.get(&"first", fingerprint(2)), None);
        assert_eq!(cache.get(&"third", fingerprint(4)), Some(40));
    }

    #[test]
    fn state_cache_preserves_the_4096_entry_boundary() {
        let mut cache = FingerprintCache::new(4096);
        for index in 0..4096 {
            cache.insert(index, fingerprint(index as u64), index);
        }
        assert_eq!(cache.get(&0, fingerprint(0)), Some(0));
        assert_eq!(cache.get(&4095, fingerprint(4095)), Some(4095));
        cache.insert(4096, fingerprint(4096), 4096);
        assert_eq!(cache.get(&0, fingerprint(0)), None);
        assert_eq!(cache.get(&4096, fingerprint(4096)), Some(4096));
    }

    #[test]
    fn full_cache_updates_existing_key_without_clearing_other_entries() {
        let mut cache = FingerprintCache::new(2);
        cache.insert("first", fingerprint(1), 10);
        cache.insert("second", fingerprint(2), 20);
        assert_eq!(cache.get(&"first", fingerprint(1)), Some(10));
        assert_eq!(cache.get(&"second", fingerprint(2)), Some(20));

        // Update an existing key when the cache is exactly at capacity (2 / 2).
        cache.insert("first", fingerprint(3), 30);
        assert_eq!(cache.get(&"first", fingerprint(3)), Some(30));
        // The unrelated entry is preserved.
        assert_eq!(cache.get(&"second", fingerprint(2)), Some(20));

        // A replacement keeps its original FIFO position.
        cache.insert("third", fingerprint(4), 40);
        assert_eq!(cache.get(&"first", fingerprint(3)), None);
        assert_eq!(cache.get(&"second", fingerprint(2)), Some(20));
        assert_eq!(cache.get(&"third", fingerprint(4)), Some(40));
    }

    #[test]
    fn eviction_follows_insertion_order_despite_lookups_and_replacements() {
        let mut cache = FingerprintCache::new(3);
        // Lexical order deliberately differs from insertion order.
        for key in ["z", "a", "m"] {
            cache.insert(key, fingerprint(1), 10);
        }
        assert_eq!(cache.get(&"a", fingerprint(2)), None);
        for _ in 0..10 {
            assert_eq!(cache.get(&"z", fingerprint(1)), Some(10));
            cache.insert("a", fingerprint(2), 20);
        }
        cache.insert("b", fingerprint(3), 30);
        assert_eq!(cache.get(&"z", fingerprint(1)), None);
        assert_eq!(cache.get(&"a", fingerprint(1)), None);
        assert_eq!(cache.get(&"a", fingerprint(2)), Some(20));
        assert_eq!(cache.get(&"m", fingerprint(1)), Some(10));
        cache.insert("c", fingerprint(4), 40);
        assert_eq!(cache.get(&"a", fingerprint(2)), None);
        assert_eq!(cache.get(&"m", fingerprint(1)), Some(10));
        assert_eq!(cache.get(&"b", fingerprint(3)), Some(30));
        assert_eq!(cache.get(&"c", fingerprint(4)), Some(40));
    }

    #[test]
    fn capacity_boundaries_preserve_all_but_the_oldest_entry() {
        for limit in [0, 1, 128, 4096] {
            let mut cache = FingerprintCache::new(limit);
            for key in 0..limit {
                cache.insert(key, fingerprint(1), key);
            }
            // Repeated replacement must neither consume slots nor reorder keys.
            for key in 0..limit {
                cache.insert(key, fingerprint(2), key + 1);
            }
            assert_eq!(cache.insertion_order.len(), limit);
            assert_eq!(
                cache
                    .insertion_order
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                limit
            );
            cache.insert(limit, fingerprint(3), limit);
            assert_eq!(cache.entries.len(), limit);
            assert_eq!(cache.insertion_order.len(), limit);
            assert_eq!(cache.get(&0, fingerprint(2)), None);
            for key in 1..limit {
                assert_eq!(cache.get(&key, fingerprint(2)), Some(key + 1));
            }
            assert_eq!(
                cache.get(&limit, fingerprint(3)),
                (limit > 0).then_some(limit)
            );
            for key in limit + 1..3 * limit + 33 {
                cache.insert(key, fingerprint(3), key);
                assert_eq!(cache.entries.len(), limit);
                assert_eq!(cache.insertion_order.len(), limit);
                assert_eq!(cache.get(&key, fingerprint(3)), (limit > 0).then_some(key));
            }
        }
    }

    #[test]
    fn fixed_trace_preserves_hot_entries_during_new_key_churn() {
        let mut cache = FingerprintCache::new(128);
        let mut old = HashMap::new();
        for key in 0..128 {
            cache.insert(key, fingerprint(1), key);
            old.insert(key, key);
        }
        let (mut hits, mut misses, mut old_hits, mut old_misses) = (0, 0, 0, 0);
        // Identical deterministic requests: one new key, then 64 recent hot keys.
        for new_key in 128..192 {
            for key in std::iter::once(new_key).chain(64..128) {
                if cache.get(&key, fingerprint(1)).is_some() {
                    hits += 1;
                } else {
                    misses += 1;
                    cache.insert(key, fingerprint(1), key);
                }
                // Previous production policy, including in-place replacement.
                if old.contains_key(&key) {
                    old_hits += 1;
                } else {
                    old_misses += 1;
                    if old.len() >= 128 {
                        old.clear();
                    }
                    old.insert(key, key);
                }
            }
        }
        eprintln!(
            "fixed trace: FIFO hits={hits} misses={misses}; full-clear hits={old_hits} misses={old_misses}"
        );
        assert_eq!((hits, misses), (4096, 64));
        assert_eq!((old_hits, old_misses), (4032, 128));
    }
}
