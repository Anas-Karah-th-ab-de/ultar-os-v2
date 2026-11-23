//! Where the kernel puts the few machine files a service manager needs.
//! (Kept in the HAL: crates above it name no kernel paths.)

use std::time::Duration;

/// The system console.
pub const CONSOLE: &str = "/dev/console";
/// Root of the memory group tree (`cgroup2` on Linux, the group tree of
/// the Ultar kernel).
pub const GROUP_ROOT: &str = "/sys/fs/cgroup";
/// Time since boot, first number in seconds.
const UPTIME: &str = "/proc/uptime";
/// The kernel's command line.
const CMDLINE: &str = "/proc/cmdline";

/// The words of the kernel's command line (empty if it is not reported).
pub fn kernel_arguments() -> Vec<String> {
    std::fs::read_to_string(CMDLINE)
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// Time since the kernel started, if the kernel reports it.
pub fn uptime() -> Option<Duration> {
    let text = std::fs::read_to_string(UPTIME).ok()?;
    let seconds: f64 = text.split_whitespace().next()?.parse().ok()?;
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
}

#[cfg(test)]
mod tests {
    #[test]
    fn uptime_is_reported_on_linux() {
        assert!(super::uptime().is_some_and(|up| up.as_secs_f64() > 0.0));
    }
}
