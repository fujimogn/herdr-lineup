//! Bounded in-memory cache for values derived from a file fingerprint.

use crate::model::Fingerprint;
use std::collections::HashMap;
use std::hash::Hash;

pub(crate) struct FingerprintCache<K, V> {
    entries: HashMap<K, (Fingerprint, V)>,
    limit: usize,
}

impl<K: Eq + Hash, V: Clone> FingerprintCache<K, V> {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            limit,
        }
    }

    pub(crate) fn get(&self, key: &K, fingerprint: Fingerprint) -> Option<V> {
        self.entries
            .get(key)
            .and_then(|(stored, value)| (*stored == fingerprint).then(|| value.clone()))
    }

    pub(crate) fn insert(&mut self, key: K, fingerprint: Fingerprint, value: V) {
        if !self.entries.contains_key(&key) && self.entries.len() >= self.limit {
            self.entries.clear();
        }
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

        // Inserting a new key at capacity clears the cache.
        cache.insert("third", fingerprint(4), 40);
        assert_eq!(cache.get(&"first", fingerprint(3)), None);
        assert_eq!(cache.get(&"second", fingerprint(2)), None);
        assert_eq!(cache.get(&"third", fingerprint(4)), Some(40));
    }
}
