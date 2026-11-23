//! Configuration and shared state of device authentication: the server's name in transcripts, the
//! device limit, the recovery cool-down, the challenge store and the clock.

use crate::challenge::ChallengeStore;
use crate::clock::Clock;
use crate::devices::Limits;
use std::sync::Arc;

/// Name used when none is configured.
pub const DEFAULT_SERVER_NAME: &str = "ultar.code-craft.tech";
/// Devices per account when not configured.
pub const DEFAULT_MAX_DEVICES: u32 = 10;
/// Upper bound of the configurable device limit.
pub const MAX_DEVICES_CEILING: u32 = 100;
/// Cool-down of a lost-device recovery when not configured (72 hours).
pub const DEFAULT_RECOVERY_COOLDOWN_SECS: i64 = 72 * 3600;
/// Shortest cool-down accepted from the environment (one hour).
pub const MIN_RECOVERY_COOLDOWN_SECS: i64 = 3600;

/// Settings of device identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceConfig {
    /// The server's name as devices derive it from their configured address (authority, lower
    /// case, default port dropped). Signatures name it, so it must match on both sides.
    pub server_name: String,
    /// Most devices in use per account.
    pub max_devices: u32,
    /// Seconds between the start of a lost-device recovery and the earliest finish.
    pub recovery_cooldown_secs: i64,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            server_name: DEFAULT_SERVER_NAME.to_string(),
            max_devices: DEFAULT_MAX_DEVICES,
            recovery_cooldown_secs: DEFAULT_RECOVERY_COOLDOWN_SECS,
        }
    }
}

/// Brings a server name into its canonical form: lower case, 1–255 characters of
/// `a-z 0-9 . - : [ ]`. `None` for anything else.
pub fn normalize_server_name(text: &str) -> Option<String> {
    let name = text.trim().to_ascii_lowercase();
    let ok = (1..=255).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-:[]".contains(&b));
    ok.then_some(name)
}

/// Parses a device limit (1 to [`MAX_DEVICES_CEILING`]).
pub fn parse_max_devices(text: &str) -> Option<u32> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|n| (1..=MAX_DEVICES_CEILING).contains(n))
}

/// Parses a recovery cool-down in seconds (at least [`MIN_RECOVERY_COOLDOWN_SECS`], at most a year).
pub fn parse_cooldown(text: &str) -> Option<i64> {
    text.trim()
        .parse::<i64>()
        .ok()
        .filter(|n| (MIN_RECOVERY_COOLDOWN_SECS..=365 * 86_400).contains(n))
}

/// Shared state of device authentication.
pub struct DeviceAuth {
    /// Pending challenges.
    pub challenges: ChallengeStore,
    /// Settings.
    pub config: DeviceConfig,
    clock: Arc<dyn Clock>,
    wall_base: i64,
}

impl DeviceAuth {
    /// Builds the state. Wall-clock time is read once; afterwards time advances with `clock`
    /// only, so it never jumps backwards and tests can drive it.
    pub fn new(config: DeviceConfig, clock: Arc<dyn Clock>) -> Self {
        let base_ms = clock.now_millis();
        let wall_base = crate::db::now() - i64::try_from(base_ms / 1000).unwrap_or(0);
        Self {
            challenges: ChallengeStore::default(),
            config,
            clock,
            wall_base,
        }
    }

    /// Milliseconds of the injected clock (for challenge lifetimes).
    pub fn now_ms(&self) -> u64 {
        self.clock.now_millis()
    }

    /// Current time as Unix seconds: the wall clock at start plus the injected clock since.
    pub fn now_secs(&self) -> i64 {
        let elapsed = i64::try_from(self.clock.now_millis() / 1000).unwrap_or(i64::MAX);
        self.wall_base.saturating_add(elapsed)
    }

    /// The per-account limits.
    pub fn limits(&self) -> Limits {
        Limits {
            max_devices: i64::from(self.config.max_devices),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    #[test]
    fn time_follows_the_injected_clock() {
        let clock = Arc::new(ManualClock::new(5_000));
        let auth = DeviceAuth::new(DeviceConfig::default(), clock.clone());
        let start = auth.now_secs();
        assert!((start - crate::db::now()).abs() <= 1);
        clock.advance_millis(90_500);
        assert_eq!(auth.now_secs(), start + 90);
        assert_eq!(auth.now_ms(), 95_500);
        assert_eq!(auth.limits().max_devices, 10);
        assert_eq!(auth.limits().quota(), 100);
    }

    #[test]
    fn server_names_are_normalised_or_refused() {
        assert_eq!(
            normalize_server_name(" Ultar.Code-Craft.tech ").as_deref(),
            Some("ultar.code-craft.tech")
        );
        assert_eq!(
            normalize_server_name("127.0.0.1:8080").as_deref(),
            Some("127.0.0.1:8080")
        );
        assert_eq!(
            normalize_server_name("[::1]:80").as_deref(),
            Some("[::1]:80")
        );
        for bad in ["", "a b", "https://x", "x/y", &"a".repeat(256), "ü.example"] {
            assert_eq!(normalize_server_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn limits_from_text() {
        assert_eq!(parse_max_devices("10"), Some(10));
        assert_eq!(parse_max_devices(" 100 "), Some(100));
        for bad in ["0", "101", "-1", "x", ""] {
            assert_eq!(parse_max_devices(bad), None, "{bad}");
        }
        assert_eq!(parse_cooldown("3600"), Some(3600));
        assert_eq!(parse_cooldown("259200"), Some(259_200));
        for bad in ["3599", "0", "-5", "99999999999", "x"] {
            assert_eq!(parse_cooldown(bad), None, "{bad}");
        }
    }
}
