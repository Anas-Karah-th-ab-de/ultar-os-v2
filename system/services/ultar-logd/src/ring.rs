//! The newest records in memory: bounded by count and by bytes.

use crate::record::Stored;
use std::collections::VecDeque;

/// A bounded queue of the newest records. When a bound is reached the
/// oldest records leave.
#[derive(Debug)]
pub struct Ring {
    records: VecDeque<Stored>,
    max_records: usize,
    max_bytes: usize,
    bytes: usize,
    evicted: u64,
}

impl Ring {
    /// A ring for at most `max_records` records (at least 1) weighing at
    /// most `max_bytes` together (one record always fits).
    pub fn new(max_records: usize, max_bytes: usize) -> Self {
        Self {
            records: VecDeque::new(),
            max_records: max_records.max(1),
            max_bytes,
            bytes: 0,
            evicted: 0,
        }
    }

    /// Adds the newest record.
    pub fn push(&mut self, record: Stored) {
        let weight = record.weight();
        while !self.records.is_empty()
            && (self.records.len() >= self.max_records || self.bytes + weight > self.max_bytes)
        {
            if let Some(old) = self.records.pop_front() {
                self.bytes -= old.weight();
                self.evicted += 1;
            }
        }
        self.bytes += weight;
        self.records.push_back(record);
    }

    /// Number of records held.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Approximate bytes held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Records that left the ring since start.
    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    /// Position of the oldest record held.
    pub fn first_seq(&self) -> Option<u64> {
        self.records.front().map(|record| record.seq)
    }

    /// The records with a position above `after_seq`, oldest first.
    pub fn after(&self, after_seq: u64) -> impl Iterator<Item = &Stored> {
        // Positions increase, so the wanted part is a suffix.
        let start = self
            .records
            .partition_point(|record| record.seq <= after_seq);
        self.records.range(start..)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::sample;

    #[test]
    fn the_count_bound_keeps_the_newest() {
        let mut ring = Ring::new(3, usize::MAX);
        assert!(ring.is_empty() && ring.first_seq().is_none());
        for seq in 1..=10 {
            ring.push(sample(seq));
            assert!(ring.len() <= 3);
        }
        let held: Vec<u64> = ring.after(0).map(|r| r.seq).collect();
        assert_eq!(held, [8, 9, 10]);
        assert_eq!((ring.first_seq(), ring.evicted()), (Some(8), 7));
        assert_eq!(ring.after(9).map(|r| r.seq).collect::<Vec<_>>(), [10]);
        assert_eq!(ring.after(10).count(), 0);
    }

    #[test]
    fn the_byte_bound_holds_and_one_record_always_fits() {
        let weight = sample(1).weight();
        let mut ring = Ring::new(1_000, weight * 2 + 1);
        for seq in 1..=50 {
            ring.push(sample(seq));
            assert!(ring.bytes() <= weight * 2 + 1 && ring.len() <= 2);
        }
        let mut tiny = Ring::new(0, 1);
        tiny.push(sample(1));
        tiny.push(sample(2));
        assert_eq!((tiny.len(), tiny.first_seq()), (1, Some(2)));
        assert_eq!(tiny.bytes(), sample(2).weight());
    }
}
