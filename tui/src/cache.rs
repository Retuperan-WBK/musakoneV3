//! Small bounded LRU for expensive render results. Values can share pixel buffers.
use std::collections::VecDeque;

pub struct Cache<K, V> {
    entries: VecDeque<(K, V, usize)>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
}

impl<K: PartialEq, V: Clone> Cache<K, V> {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            max_entries,
            max_bytes,
        }
    }

    pub fn get<Q: PartialEq + ?Sized>(&mut self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
    {
        let index = self
            .entries
            .iter()
            .position(|(k, _, _)| k.borrow() == key)?;
        let entry = self.entries.remove(index)?;
        let value = entry.1.clone();
        self.entries.push_back(entry);
        Some(value)
    }

    pub fn insert(&mut self, key: K, value: V, bytes: usize) {
        if bytes > self.max_bytes || self.max_entries == 0 {
            return;
        }
        if let Some(i) = self.entries.iter().position(|(k, _, _)| *k == key) {
            self.bytes -= self.entries.remove(i).unwrap().2;
        }
        while self.entries.len() >= self.max_entries || self.bytes + bytes > self.max_bytes {
            self.bytes -= self.entries.pop_front().unwrap().2;
        }
        self.bytes += bytes;
        self.entries.push_back((key, value, bytes));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_memory_and_evicts_least_recently_used() {
        let mut cache = Cache::new(2, 10);
        cache.insert("a", 1, 4);
        cache.insert("b", 2, 4);
        assert_eq!(cache.get(&"a"), Some(1));
        cache.insert("c", 3, 4);
        assert_eq!(cache.get(&"b"), None);
        cache.insert("c", 4, 7);
        assert_eq!(cache.get(&"a"), None);
        assert_eq!(cache.get(&"c"), Some(4));
        cache.insert("huge", 5, 11);
        assert_eq!(cache.get(&"huge"), None);
        cache.clear();
        assert_eq!(cache.get(&"c"), None);
    }
}
