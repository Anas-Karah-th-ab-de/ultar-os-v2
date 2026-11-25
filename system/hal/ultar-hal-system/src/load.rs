//! Processor load and memory in use (the phone's device card, monitors), read from
//! `/proc/stat` and `/proc/meminfo` where the kernel offers them – best effort: without them
//! the values are `None`.

/// Busy and total jiffies of all processors (first line of `/proc/stat`).
pub fn cpu_times(stat: &str) -> Option<(u64, u64)> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let values: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();
    if values.len() < 4 {
        return None;
    }
    let total: u64 = values.iter().sum();
    // idle + iowait are not busy.
    let idle = values[3] + values.get(4).copied().unwrap_or(0);
    Some((total.saturating_sub(idle), total))
}

/// Load in percent between two readings of [`cpu_times`].
pub fn cpu_percent(before: (u64, u64), after: (u64, u64)) -> u8 {
    let busy = after.0.saturating_sub(before.0);
    let total = after.1.saturating_sub(before.1);
    if total == 0 {
        return 0;
    }
    (busy * 100 / total).min(100) as u8
}

/// Memory in use in percent (`MemTotal` − `MemAvailable`).
pub fn ram_percent(meminfo: &str) -> Option<u8> {
    let field = |name: &str| {
        meminfo
            .lines()
            .find(|l| l.starts_with(name))?
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()
    };
    let total = field("MemTotal:")?;
    let available = field("MemAvailable:")?;
    if total == 0 {
        return None;
    }
    Some((total.saturating_sub(available) * 100 / total).min(100) as u8)
}

/// Reads both values; keeps the last processor reading for the next call.
#[derive(Debug, Default)]
pub struct Sampler {
    last: Option<(u64, u64)>,
}

impl Sampler {
    /// Processor and memory in percent (`None` where the kernel has no such file).
    pub fn sample(&mut self) -> (Option<u8>, Option<u8>) {
        let stat = std::fs::read_to_string("/proc/stat").ok();
        let now = stat.as_deref().and_then(cpu_times);
        let cpu = match (self.last, now) {
            (Some(a), Some(b)) => Some(cpu_percent(a, b)),
            _ => None,
        };
        self.last = now.or(self.last);
        let ram = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .as_deref()
            .and_then(ram_percent);
        (cpu, ram)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processor_load_from_two_readings() {
        let a = cpu_times("cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 2 3 4\n").unwrap();
        let b = cpu_times("cpu  200 0 200 800 100 0 0 0 0 0\n").unwrap();
        assert_eq!(a, (200, 1000));
        assert_eq!(cpu_percent(a, b), 66);
        assert_eq!(cpu_percent(a, a), 0);
        assert!(cpu_times("nothing").is_none());
    }

    #[test]
    fn memory_in_use() {
        let m = "MemTotal:       8000 kB\nMemFree:  10 kB\nMemAvailable:   6000 kB\n";
        assert_eq!(ram_percent(m), Some(25));
        assert_eq!(ram_percent("MemTotal: 0 kB\nMemAvailable: 0 kB"), None);
    }
}
