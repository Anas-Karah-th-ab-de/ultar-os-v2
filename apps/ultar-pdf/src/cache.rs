//! A least-recently-used picture cache with a byte budget (rendered pages and thumbnails).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use ultar_office::pdf::PageImage;

/// Which picture: a page at a scale (the scale's bits, so the key is exact).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub page: usize,
    pub scale: u32,
}

impl Key {
    pub fn new(page: usize, scale: f32) -> Key {
        Key {
            page,
            scale: scale.to_bits(),
        }
    }
}

/// Pictures by key; the least recently used go first when the budget is exceeded. The newest
/// picture always stays, even when it alone is larger than the budget.
pub struct Cache {
    budget: usize,
    bytes: usize,
    map: HashMap<Key, Arc<PageImage>>,
    order: VecDeque<Key>,
}

impl Cache {
    pub fn new(budget: usize) -> Cache {
        Cache {
            budget,
            bytes: 0,
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// The picture, marked as used now.
    pub fn get(&mut self, k: Key) -> Option<Arc<PageImage>> {
        let img = self.map.get(&k).cloned()?;
        self.order.retain(|o| *o != k);
        self.order.push_back(k);
        Some(img)
    }

    /// Whether the picture is there (not marked as used).
    pub fn has(&self, k: Key) -> bool {
        self.map.contains_key(&k)
    }

    /// The newest picture of `page` at any scale (shown scaled until the right one arrives).
    pub fn newest_of(&self, page: usize) -> Option<Arc<PageImage>> {
        self.order
            .iter()
            .rev()
            .find(|k| k.page == page)
            .and_then(|k| self.map.get(k).cloned())
    }

    pub fn insert(&mut self, k: Key, img: Arc<PageImage>) {
        if let Some(old) = self.map.insert(k, img.clone()) {
            self.bytes = self.bytes.saturating_sub(old.rgba.len());
            self.order.retain(|o| *o != k);
        }
        self.bytes += img.rgba.len();
        self.order.push_back(k);
        while self.bytes > self.budget && self.order.len() > 1 {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(dropped) = self.map.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(dropped.rgba.len());
            }
        }
    }

    /// Bytes held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Pictures held.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(bytes: usize) -> Arc<PageImage> {
        Arc::new(PageImage {
            w: 1,
            h: (bytes / 4) as u32,
            rgba: vec![255; bytes],
        })
    }

    #[test]
    fn the_budget_evicts_the_least_recently_used() {
        let mut c = Cache::new(1000);
        c.insert(Key::new(0, 1.0), img(400));
        c.insert(Key::new(1, 1.0), img(400));
        // page 0 is used again, so page 1 is the oldest
        assert!(c.get(Key::new(0, 1.0)).is_some());
        c.insert(Key::new(2, 1.0), img(400));
        assert!(c.has(Key::new(0, 1.0)));
        assert!(!c.has(Key::new(1, 1.0)));
        assert!(c.has(Key::new(2, 1.0)));
        assert!(c.bytes() <= 1000);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn the_newest_picture_stays_even_over_the_budget() {
        let mut c = Cache::new(100);
        c.insert(Key::new(0, 1.0), img(400));
        assert_eq!(c.len(), 1);
        c.insert(Key::new(1, 1.0), img(400));
        assert_eq!((c.len(), c.has(Key::new(1, 1.0))), (1, true));
    }

    #[test]
    fn inserting_a_key_twice_counts_its_bytes_once() {
        let mut c = Cache::new(10_000);
        c.insert(Key::new(0, 1.0), img(400));
        c.insert(Key::new(0, 1.0), img(400));
        assert_eq!((c.bytes(), c.len()), (400, 1));
    }

    #[test]
    fn the_newest_of_a_page_is_found_at_any_scale() {
        let mut c = Cache::new(10_000);
        c.insert(Key::new(3, 1.0), img(40));
        c.insert(Key::new(3, 2.0), img(160));
        c.insert(Key::new(4, 2.0), img(160));
        assert_eq!(c.newest_of(3).map(|i| i.rgba.len()), Some(160));
        assert!(c.newest_of(9).is_none());
        assert!(!c.is_empty());
    }
}
