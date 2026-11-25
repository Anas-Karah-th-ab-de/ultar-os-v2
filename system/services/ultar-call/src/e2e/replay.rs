//! Replay window for counters of sealed packets: a counter is taken once, and one more than
//! [`SPAN`] behind the highest seen is too old (media packets may come out of order).

/// How far behind the newest counter a packet may still arrive.
pub const SPAN: u64 = 1024;

/// Seen counters near the top.
#[derive(Default)]
pub struct Window {
    top: Option<u64>,
    seen: Vec<u64>,
}

impl Window {
    /// True when `c` was not seen and is not too old (does not mark it).
    pub fn fresh(&self, c: u64) -> bool {
        match self.top {
            None => true,
            Some(top) if c > top => true,
            Some(top) => top - c < SPAN && !self.seen.contains(&c),
        }
    }

    /// Marks `c` as taken (after its message was authenticated).
    pub fn mark(&mut self, c: u64) {
        let top = self.top.map_or(c, |t| t.max(c));
        self.top = Some(top);
        self.seen.push(c);
        self.seen.retain(|s| top - s < SPAN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_counter_once_and_not_too_old() {
        let mut w = Window::default();
        for c in [5, 3, 9] {
            assert!(w.fresh(c));
            w.mark(c);
            assert!(!w.fresh(c));
        }
        assert!(w.fresh(4) && w.fresh(10));
        w.mark(5000);
        assert!(!w.fresh(9), "too far behind");
        assert!(w.fresh(4999));
    }
}
