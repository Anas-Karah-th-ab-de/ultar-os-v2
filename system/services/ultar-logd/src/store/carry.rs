//! Fair retention: which records of an evicted segment are stored again.
//!
//! When the size limit forces the oldest segment out, its records are
//! looked at before the file goes:
//!
//! * records of the **largest consumer** (the source with the most bytes
//!   on disk) are never carried – its own volume is what is evicted;
//! * records of every other source are carried while that source stays
//!   within its **share** ([`SHARE_DIVISOR`]: an eighth of the total) –
//!   warnings and errors first, then the rest, oldest first;
//! * one eviction carries at most half a segment, so every eviction frees
//!   room;
//! * records older than the age limit are not carried.
//!
//! Carried records are stored again through the log service's core, with a
//! new position and the field `carried` = original position (when the
//! record has room for a field); time, source, level, key and fields stay.
//! They are ordinary records of the hash chain.

use super::open::frame_bytes;
use super::SegmentStore;
use crate::record::Stored;
use crate::segment::scan;
use ultar_service::log::Level;

/// A source may keep at most `total_bytes / SHARE_DIVISOR` on disk through
/// carrying.
pub const SHARE_DIVISOR: u64 = 8;

impl SegmentStore {
    /// The source with the most bytes on disk.
    fn largest_consumer(&self) -> Option<&str> {
        self.sources
            .iter()
            .max_by_key(|(_, bytes)| **bytes)
            .map(|(name, _)| name.as_str())
    }

    /// Chooses the records of the oldest segment to carry forward.
    pub(super) fn select_carry(&self, now_ms: u64) -> Vec<Stored> {
        let Some(oldest) = self.segments.front() else {
            return Vec::new();
        };
        let largest = self.largest_consumer().unwrap_or("").to_owned();
        if oldest.sources.keys().all(|name| *name == largest) {
            // Nothing but the largest consumer: no need to read the file.
            return Vec::new();
        }
        let limit_bytes = self.config.segment_bytes.saturating_add(4096);
        let Ok(data) = self.storage.read(&oldest.name, limit_bytes) else {
            return Vec::new();
        };
        let age_limit = match self.config.max_age_ms {
            0 => 0,
            age => now_ms.saturating_sub(age),
        };
        let mut candidates: Vec<Stored> = scan(&self.chain, &data)
            .records
            .into_iter()
            .filter(|record| record.service != largest && record.time_ms >= age_limit)
            .collect();
        // Warnings and errors first; within a class the oldest first.
        candidates.sort_by_key(|record| (record.level < Level::Warn, record.seq));
        self.within_budget(candidates, &oldest.sources)
    }

    /// Keeps the candidates that fit the per-source share and the
    /// per-eviction budget. `leaving` are the bytes per source of the
    /// segment that is about to be removed.
    fn within_budget(
        &self,
        candidates: Vec<Stored>,
        leaving: &std::collections::BTreeMap<String, u64>,
    ) -> Vec<Stored> {
        let share = self.config.total_bytes / SHARE_DIVISOR;
        let mut budget = self.config.segment_bytes / 2;
        let mut kept_by_source = std::collections::BTreeMap::<&str, u64>::new();
        let mut kept = Vec::new();
        for record in &candidates {
            let bytes = frame_bytes(record);
            let on_disk = self
                .sources
                .get(&record.service)
                .copied()
                .unwrap_or(0)
                .saturating_sub(leaving.get(&record.service).copied().unwrap_or(0));
            let already = kept_by_source.entry(record.service.as_str()).or_insert(0);
            if bytes > budget || on_disk + *already + bytes > share {
                continue;
            }
            budget -= bytes;
            *already += bytes;
            kept.push(record.clone());
        }
        kept
    }

    /// Records taken out of evicted segments since the last call; the
    /// caller stores them again (see the module documentation).
    pub fn take_carried(&mut self) -> Vec<Stored> {
        std::mem::take(&mut self.carried)
    }
}
