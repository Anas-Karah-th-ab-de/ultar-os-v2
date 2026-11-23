//! The kernel log as a source.
//!
//! The Ultar kernel offers its log as a file that always shows the newest
//! part of a ring buffer (64 KiB); there is no event interface. The log
//! service therefore reads the file when it has a reason to (start,
//! reload, a query) and [`KernelTail`] finds the lines that are new since
//! the last read. Lines of the kernel are free text and are stored as the
//! separate source `kernel`; only readers with `log.read_all` get them.

use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;

/// Lines remembered to find the continuation in the next snapshot.
const ANCHOR_LINES: usize = 8;
/// Most new lines taken from one snapshot.
pub const MAX_LINES_PER_READ: usize = 1024;
/// Largest snapshot read, in bytes.
pub const MAX_SNAPSHOT_BYTES: u64 = 256 * 1024;

/// Finds the new lines in successive snapshots of a ring buffer.
#[derive(Debug, Default)]
pub struct KernelTail {
    anchor: VecDeque<String>,
    started: bool,
}

impl KernelTail {
    /// A tail that has seen nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the lines of `snapshot` that come after what the previous
    /// snapshot ended with. If the previous end cannot be found (the ring
    /// wrapped completely in between) the whole snapshot is new.
    pub fn update(&mut self, snapshot: &str) -> Vec<String> {
        let lines: Vec<&str> = snapshot.lines().filter(|line| !line.is_empty()).collect();
        let start = if self.started {
            self.continuation(&lines)
        } else {
            0
        };
        self.started = true;
        let fresh = &lines[start..];
        // If more arrived than is taken, the newest lines win.
        let fresh = &fresh[fresh.len().saturating_sub(MAX_LINES_PER_READ)..];
        for line in fresh {
            if self.anchor.len() == ANCHOR_LINES {
                self.anchor.pop_front();
            }
            self.anchor.push_back((*line).to_owned());
        }
        fresh.iter().map(|line| (*line).to_owned()).collect()
    }

    /// Index of the first line after the last occurrence of the anchor.
    fn continuation(&self, lines: &[&str]) -> usize {
        if self.anchor.is_empty() {
            return 0;
        }
        let needle = self.anchor.len();
        // The ring may have dropped old lines: try the full anchor first,
        // then ever shorter ends of it.
        for skip in 0..needle {
            let wanted = needle - skip;
            if lines.len() < wanted {
                continue;
            }
            for end in (wanted..=lines.len()).rev() {
                let window = &lines[end - wanted..end];
                if window
                    .iter()
                    .zip(self.anchor.iter().skip(skip))
                    .all(|(a, b)| a == b)
                {
                    return end;
                }
            }
        }
        0
    }
}

/// Reads the kernel log file (bounded). `None` when it cannot be read.
pub fn read_snapshot(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_SNAPSHOT_BYTES).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(range: std::ops::Range<u32>) -> String {
        range.map(|n| format!("[kern] line {n}\n")).collect()
    }

    #[test]
    fn the_first_snapshot_is_new_and_a_repeated_one_is_not() {
        let mut tail = KernelTail::new();
        assert_eq!(tail.update(&text(0..5)).len(), 5);
        assert!(tail.update(&text(0..5)).is_empty());
        assert!(tail.update("").is_empty());
    }

    #[test]
    fn only_the_lines_after_the_known_end_are_new() {
        let mut tail = KernelTail::new();
        tail.update(&text(0..20));
        let fresh = tail.update(&text(0..23));
        assert_eq!(
            fresh,
            ["[kern] line 20", "[kern] line 21", "[kern] line 22"]
        );
        // The ring moved on: old lines left at the front.
        let fresh = tail.update(&text(15..30));
        assert_eq!(fresh.len(), 7);
        assert_eq!(fresh[0], "[kern] line 23");
    }

    #[test]
    fn a_ring_that_wrapped_completely_is_all_new() {
        let mut tail = KernelTail::new();
        tail.update(&text(0..10));
        assert_eq!(tail.update(&text(500..520)).len(), 20);
        assert!(tail.update(&text(500..520)).is_empty());
    }

    #[test]
    fn repeated_identical_lines_do_not_confuse_the_tail() {
        let mut tail = KernelTail::new();
        let same = "tick\n".repeat(30);
        assert_eq!(tail.update(&same).len(), 30);
        // The same 30 lines again: nothing is provably new.
        assert!(tail.update(&same).is_empty());
        let more = format!("{same}tock\n");
        assert_eq!(tail.update(&more), ["tock"]);
    }

    #[test]
    fn a_short_ring_keeps_only_the_end_of_the_anchor() {
        let mut tail = KernelTail::new();
        tail.update(&text(0..10));
        // Only the last two known lines are still there, then two new ones.
        let fresh = tail.update(&text(8..12));
        assert_eq!(fresh, ["[kern] line 10", "[kern] line 11"]);
    }

    #[test]
    fn a_burst_is_capped_to_the_newest_lines() {
        let mut tail = KernelTail::new();
        let fresh = tail.update(&text(0..(MAX_LINES_PER_READ as u32 + 50)));
        assert_eq!(fresh.len(), MAX_LINES_PER_READ);
        assert_eq!(fresh[0], "[kern] line 50");
    }

    #[test]
    fn snapshots_are_read_bounded_and_missing_files_are_none() {
        let path = std::env::temp_dir().join(format!("ulogd-kern-{}", std::process::id()));
        std::fs::write(&path, b"a\n\xff\xfeb\n").unwrap();
        let snapshot = read_snapshot(&path).unwrap();
        assert!(snapshot.starts_with("a\n") && snapshot.ends_with("b\n"));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_snapshot(&path), None);
    }
}
