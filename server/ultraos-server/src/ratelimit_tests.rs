//! Unit tests for the token-bucket limiter (manual clock, no sleeping).

use super::*;
use crate::clock::ManualClock;
use std::net::{Ipv4Addr, Ipv6Addr};

fn limiter(capacity: u32, per_minute: u32, max_entries: usize) -> (RateLimiter, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(0));
    let b = BucketConfig {
        capacity,
        refill_per_minute: per_minute,
    };
    let cfg = LimiterConfig {
        general: b,
        download: b,
        auth: b,
        max_entries,
    };
    (RateLimiter::new(cfg, clock.clone()), clock)
}

fn v4(n: u32) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from(n))
}

#[test]
fn burst_then_limited_then_recovers() {
    let (l, clock) = limiter(3, 60, 100);
    for _ in 0..3 {
        assert_eq!(l.check(v4(1), LimitClass::General), Decision::Allowed);
    }
    assert_eq!(
        l.check(v4(1), LimitClass::General),
        Decision::Limited {
            retry_after_secs: 1
        }
    );
    clock.advance_millis(999);
    assert!(matches!(
        l.check(v4(1), LimitClass::General),
        Decision::Limited { .. }
    ));
    clock.advance_millis(1);
    assert_eq!(l.check(v4(1), LimitClass::General), Decision::Allowed);
    assert!(matches!(
        l.check(v4(1), LimitClass::General),
        Decision::Limited { .. }
    ));
}

#[test]
fn refill_is_exact_under_frequent_polling() {
    // 1 token per minute; polling every millisecond must not lose fractions.
    let (l, clock) = limiter(1, 1, 100);
    assert_eq!(l.check(v4(1), LimitClass::General), Decision::Allowed);
    for _ in 0..59_999 {
        clock.advance_millis(1);
        assert!(matches!(
            l.check(v4(1), LimitClass::General),
            Decision::Limited { .. }
        ));
    }
    clock.advance_millis(1);
    assert_eq!(l.check(v4(1), LimitClass::General), Decision::Allowed);
}

#[test]
fn retry_after_reports_the_real_wait() {
    let (l, clock) = limiter(1, 6, 100);
    assert_eq!(l.check(v4(1), LimitClass::Auth), Decision::Allowed);
    assert_eq!(
        l.check(v4(1), LimitClass::Auth),
        Decision::Limited {
            retry_after_secs: 10
        }
    );
    clock.advance_millis(4_500);
    assert_eq!(
        l.check(v4(1), LimitClass::Auth),
        Decision::Limited {
            retry_after_secs: 6
        }
    );
}

#[test]
fn clients_and_classes_are_independent() {
    let (l, _clock) = limiter(1, 1, 100);
    assert_eq!(l.check(v4(1), LimitClass::General), Decision::Allowed);
    assert!(matches!(
        l.check(v4(1), LimitClass::General),
        Decision::Limited { .. }
    ));
    assert_eq!(l.check(v4(2), LimitClass::General), Decision::Allowed);
    assert_eq!(l.check(v4(1), LimitClass::Download), Decision::Allowed);
    assert_eq!(l.check(v4(1), LimitClass::Auth), Decision::Allowed);
    assert_eq!(l.len(), 4);
}

#[test]
fn ipv6_is_keyed_by_slash_64_and_mapped_v4_by_address() {
    let (l, _clock) = limiter(1, 1, 100);
    let a = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 2, 0, 0, 0, 1));
    let same_net = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 2, 9, 9, 9, 9));
    let other_net = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 3, 0, 0, 0, 1));
    assert_eq!(l.check(a, LimitClass::General), Decision::Allowed);
    assert!(matches!(
        l.check(same_net, LimitClass::General),
        Decision::Limited { .. }
    ));
    assert_eq!(l.check(other_net, LimitClass::General), Decision::Allowed);
    let mapped = IpAddr::V6(Ipv4Addr::new(10, 0, 0, 1).to_ipv6_mapped());
    assert_eq!(l.check(mapped, LimitClass::General), Decision::Allowed);
    let plain = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
    assert!(matches!(
        l.check(plain, LimitClass::General),
        Decision::Limited { .. }
    ));
}

#[test]
fn refilled_buckets_are_evicted_within_window_plus_sweep_period() {
    let (l, clock) = limiter(2, 60, 100);
    let window = l.config().general.window_millis();
    assert_eq!(window, 2_000);
    assert_eq!(l.check(v4(1), LimitClass::General), Decision::Allowed);
    assert_eq!(l.len(), 1);
    clock.advance_millis(999);
    assert_eq!(l.sweep(), 0, "bucket is still below capacity");
    clock.advance_millis(1);
    assert_eq!(l.sweep(), 1);
    assert!(l.is_empty());
}

#[test]
fn check_sweeps_automatically_after_the_sweep_period() {
    let (l, clock) = limiter(1, 60, 100);
    for n in 0..10 {
        assert_eq!(l.check(v4(n), LimitClass::General), Decision::Allowed);
    }
    assert_eq!(l.len(), 10);
    clock.advance_millis(SWEEP_PERIOD_MS);
    assert_eq!(l.check(v4(1000), LimitClass::General), Decision::Allowed);
    assert_eq!(
        l.len(),
        1,
        "old refilled buckets are gone, only the new client remains"
    );
}

#[test]
fn table_is_bounded_and_overflow_clients_share_one_bucket() {
    let (l, clock) = limiter(2, 1, 5);
    for n in 0..5 {
        assert_eq!(l.check(v4(n), LimitClass::General), Decision::Allowed);
    }
    assert_eq!(l.len(), 5);
    clock.advance_millis(FULL_SWEEP_PERIOD_MS);
    // Table full of live buckets: new clients go to the shared overflow bucket (capacity 2).
    assert_eq!(l.check(v4(100), LimitClass::General), Decision::Allowed);
    assert_eq!(l.check(v4(101), LimitClass::General), Decision::Allowed);
    assert!(matches!(
        l.check(v4(102), LimitClass::General),
        Decision::Limited { .. }
    ));
    assert_eq!(l.len(), 5, "the table never grows beyond max_entries");
    // Known clients keep their own bucket.
    assert_eq!(l.check(v4(0), LimitClass::General), Decision::Allowed);
    // A flood of new addresses neither grows the table nor resets anyone.
    for n in 1000..3000 {
        let _ = l.check(v4(n), LimitClass::General);
    }
    assert_eq!(l.len(), 5);
    assert!(matches!(
        l.check(v4(0), LimitClass::General),
        Decision::Limited { .. }
    ));
}

#[test]
fn full_table_frees_slots_once_buckets_refill() {
    let (l, clock) = limiter(1, 60, 2);
    assert_eq!(l.check(v4(1), LimitClass::General), Decision::Allowed);
    assert_eq!(l.check(v4(2), LimitClass::General), Decision::Allowed);
    clock.advance_millis(1_000);
    assert_eq!(l.check(v4(3), LimitClass::General), Decision::Allowed);
    assert_eq!(
        l.len(),
        1,
        "both old buckets were refilled and evicted, v4(3) took a slot"
    );
}

#[test]
fn zero_limits_are_treated_as_one() {
    let (l, clock) = limiter(0, 0, 10);
    assert_eq!(l.check(v4(1), LimitClass::General), Decision::Allowed);
    assert_eq!(
        l.check(v4(1), LimitClass::General),
        Decision::Limited {
            retry_after_secs: 60
        }
    );
    clock.advance_millis(60_000);
    assert_eq!(l.check(v4(1), LimitClass::General), Decision::Allowed);
}

#[test]
fn default_config_is_sane() {
    let cfg = LimiterConfig::default();
    assert!(cfg.max_entries > 0);
    assert!(cfg.auth.capacity < cfg.general.capacity);
    assert_eq!(cfg.general.window_millis(), 60_000);
}
