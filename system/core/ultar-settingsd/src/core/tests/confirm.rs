//! Confirm-or-revert flows, driven by the injected clock.

use super::super::testkit::*;
use super::super::*;
use crate::apply::ApplyError;
use crate::value::Value;
use std::time::Duration;

const MODE: &str = "display.mode";
const SCALE: &str = "display.boe-ne135.scale";

fn pending(key: &str, value: Value, previous: Value) -> Event {
    Event::Pending {
        key: key.to_owned(),
        value,
        previous,
        seconds: 15,
    }
}

fn resolved(key: &str, outcome: PendingOutcome, value: Value) -> Event {
    Event::Resolved {
        key: key.to_owned(),
        outcome,
        value,
    }
}

#[test]
fn provisional_value_takes_effect_and_announces_its_deadline() {
    let kit = Kit::new("confirm-provisional");
    let mut core = kit.open_with_panel();
    assert_eq!(core.set(&user(), MODE, Value::text("mirror")), Ok(true));
    assert_eq!(core.get(MODE), Ok(Value::text("mirror")));
    assert_eq!(
        core.take_events(),
        [
            changed(MODE, Value::text("mirror")),
            pending(MODE, Value::text("mirror"), Value::text("extend")),
        ]
    );
    let calls = kit.applier.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].provisional);
    assert_eq!(core.pending_keys(), [MODE]);
    assert_eq!(core.next_deadline(), Some(Duration::from_secs(15)));
}

#[test]
fn confirm_keeps_the_value() {
    let kit = Kit::new("confirm-keep");
    let mut core = kit.open_with_panel();
    core.set(&user(), MODE, Value::text("mirror")).unwrap();
    core.take_events();
    kit.advance(14);
    core.tick().unwrap();
    assert_eq!(core.confirm(Some(MODE)), Ok(1));
    assert_eq!(
        core.take_events(),
        [resolved(MODE, PendingOutcome::Kept, Value::text("mirror"))]
    );
    assert!(core.pending_keys().is_empty());
    // Long after the deadline the value is still there, also after a restart.
    kit.advance(3600);
    core.tick().unwrap();
    assert_eq!(core.get(MODE), Ok(Value::text("mirror")));
    assert!(core.take_events().is_empty());
    drop(core);
    let core = kit.open();
    assert_eq!(core.get(MODE), Ok(Value::text("mirror")));
}

#[test]
fn timeout_restores_the_previous_value_and_announces_it() {
    let kit = Kit::new("confirm-timeout");
    let mut core = kit.open_with_panel();
    core.set(&user(), MODE, Value::text("mirror")).unwrap();
    core.take_events();
    kit.applier.clear_calls();
    kit.clock.advance(Duration::from_millis(14_999));
    core.tick().unwrap();
    assert_eq!(core.get(MODE), Ok(Value::text("mirror")));
    kit.clock.advance(Duration::from_millis(1));
    core.tick().unwrap();
    assert_eq!(core.get(MODE), Ok(Value::text("extend")));
    assert_eq!(
        core.take_events(),
        [
            changed(MODE, Value::text("extend")),
            resolved(MODE, PendingOutcome::TimedOut, Value::text("extend")),
        ]
    );
    let calls = kit.applier.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].value, Value::text("extend"));
    assert!(!calls[0].provisional);
    assert_eq!(core.confirm(Some(MODE)), Err(SettingsError::NotPending));
}

#[test]
fn explicit_revert_restores_at_once() {
    let kit = Kit::new("confirm-revert");
    let mut core = kit.open_with_panel();
    core.set(&user(), SCALE, Value::Int(150)).unwrap();
    core.take_events();
    assert_eq!(core.revert(Some(SCALE)), Ok(1));
    assert_eq!(core.get(SCALE), Ok(Value::Int(200)));
    assert_eq!(
        core.take_events(),
        [
            changed(SCALE, Value::Int(200)),
            resolved(SCALE, PendingOutcome::Reverted, Value::Int(200)),
        ]
    );
    assert_eq!(core.revert(Some(SCALE)), Err(SettingsError::NotPending));
    assert_eq!(
        core.revert(Some("no.such.key")),
        Err(SettingsError::UnknownKey)
    );
    assert_eq!(core.revert(None), Ok(0));
    assert_eq!(core.confirm(None), Ok(0));
}

#[test]
fn a_stored_value_comes_back_not_the_default() {
    let kit = Kit::new("confirm-stored");
    let mut core = kit.open_with_panel();
    core.set(&user(), SCALE, Value::Int(150)).unwrap();
    core.confirm(None).unwrap();
    core.set(&user(), SCALE, Value::Int(300)).unwrap();
    kit.advance(15);
    core.tick().unwrap();
    assert_eq!(core.get(SCALE), Ok(Value::Int(150)));
}

#[test]
fn changing_again_while_pending_restarts_the_clock_and_keeps_the_original() {
    let kit = Kit::new("confirm-again");
    let mut core = kit.open_with_panel();
    core.set(&user(), SCALE, Value::Int(150)).unwrap();
    kit.advance(10);
    core.set(&user(), SCALE, Value::Int(175)).unwrap();
    let events = core.take_events();
    assert_eq!(events[3], pending(SCALE, Value::Int(175), Value::Int(200)));
    kit.advance(10);
    core.tick().unwrap();
    // 20 s after the first change, but only 10 s after the second.
    assert_eq!(core.get(SCALE), Ok(Value::Int(175)));
    kit.advance(5);
    core.tick().unwrap();
    assert_eq!(core.get(SCALE), Ok(Value::Int(200)));
}

#[test]
fn setting_the_previous_value_back_is_a_revert() {
    let kit = Kit::new("confirm-back");
    let mut core = kit.open_with_panel();
    core.set(&user(), MODE, Value::text("mirror")).unwrap();
    core.take_events();
    assert_eq!(core.set(&user(), MODE, Value::text("extend")), Ok(true));
    assert!(core.pending_keys().is_empty());
    assert_eq!(
        core.take_events(),
        [
            changed(MODE, Value::text("extend")),
            resolved(MODE, PendingOutcome::Reverted, Value::text("extend")),
        ]
    );
    assert_eq!(core.next_deadline(), None);
}

#[test]
fn several_pending_changes_resolve_independently_or_together() {
    let kit = Kit::new("confirm-several");
    let mut core = kit.open_with_panel();
    core.set(&user(), MODE, Value::text("mirror")).unwrap();
    kit.advance(5);
    core.set(&user(), SCALE, Value::Int(150)).unwrap();
    core.set(&admin(), "privacy.network", Value::Bool(false))
        .unwrap();
    assert_eq!(core.pending_keys(), [SCALE, MODE, "privacy.network"]);
    assert_eq!(core.confirm(Some(SCALE)), Ok(1));
    kit.advance(10);
    core.tick().unwrap();
    // The mode ran out; the network switch has 5 s left.
    assert_eq!(core.get(MODE), Ok(Value::text("extend")));
    assert_eq!(core.get("privacy.network"), Ok(Value::Bool(false)));
    assert_eq!(core.confirm(None), Ok(1));
    kit.advance(60);
    core.tick().unwrap();
    assert_eq!(core.get("privacy.network"), Ok(Value::Bool(false)));
    assert_eq!(core.get(SCALE), Ok(Value::Int(150)));
}

#[test]
fn reset_of_a_risky_setting_is_provisional_too() {
    let kit = Kit::new("confirm-reset");
    let mut core = kit.open_with_panel();
    core.set(&user(), SCALE, Value::Int(150)).unwrap();
    core.confirm(None).unwrap();
    core.take_events();
    assert_eq!(core.reset(&user(), SCALE), Ok(true));
    assert_eq!(core.get(SCALE), Ok(Value::Int(200)));
    assert_eq!(
        core.take_events()[1],
        pending(SCALE, Value::Int(200), Value::Int(150))
    );
    kit.advance(15);
    core.tick().unwrap();
    assert_eq!(core.get(SCALE), Ok(Value::Int(150)));
    // Confirmed this time: the stored value is gone, the default tracks the device.
    core.reset(&user(), SCALE).unwrap();
    core.confirm(Some(SCALE)).unwrap();
    core.flush().unwrap();
    let text = std::fs::read_to_string(kit.dir.path().join("displays.toml")).unwrap();
    assert!(!text.contains("scale"), "{text}");
}

#[test]
fn the_number_of_pending_changes_is_bounded() {
    let kit = Kit::new("confirm-limit");
    let mut core = kit.open();
    let displays: Vec<_> = (0..12)
        .map(|n| display("MON", &format!("M{n}"), &format!("DP-{n}"), false))
        .collect();
    let ids = core.set_displays(&admin(), &displays).unwrap().ids;
    let mut accepted = 0;
    for id in &ids {
        for (item, value) in [
            ("scale", Value::Int(125)),
            ("orientation", Value::text("portrait")),
            ("position_y", Value::Int(7)),
        ] {
            match core.set(&user(), &format!("display.{id}.{item}"), value) {
                Ok(true) => accepted += 1,
                Err(SettingsError::Limit) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }
    assert_eq!(accepted, MAX_PENDING);
    assert_eq!(core.pending_keys().len(), MAX_PENDING);
    // A pending key can still be changed; room returns after confirming.
    let first = core.pending_keys().remove(0);
    assert!(core.reset(&user(), &first).is_ok());
    core.confirm(None).unwrap();
    assert_eq!(core.set(&user(), MODE, Value::text("mirror")), Ok(true));
}

#[test]
fn failing_applier_refuses_the_provisional_value_cleanly() {
    let kit = Kit::new("confirm-apply-fail");
    let mut core = kit.open_with_panel();
    kit.applier.fail_next(ApplyError::Unsupported);
    assert_eq!(
        core.set(&user(), MODE, Value::text("mirror")),
        Err(SettingsError::Apply(ApplyError::Unsupported))
    );
    assert_eq!(core.get(MODE), Ok(Value::text("extend")));
    assert!(core.pending_keys().is_empty());
    assert!(core.take_events().is_empty());
    // Nothing is left in the journal either: a restart reverts nothing.
    drop(core);
    let mut core = kit.open();
    assert!(core.take_events().is_empty());
}

#[test]
fn failing_applier_on_revert_is_reported_and_counted() {
    let kit = Kit::new("confirm-revert-fail");
    let mut core = kit.open_with_panel();
    core.set(&user(), MODE, Value::text("mirror")).unwrap();
    core.take_events();
    kit.applier.fail_next(ApplyError::Busy);
    kit.advance(15);
    assert_eq!(core.tick(), Err(SettingsError::Apply(ApplyError::Busy)));
    // The service's view is consistent: old value, nothing pending, announced.
    assert_eq!(core.get(MODE), Ok(Value::text("extend")));
    assert!(core.pending_keys().is_empty());
    assert_eq!(core.take_events().len(), 2);
    assert_eq!(core.stats().apply_failures, 1);
    assert_eq!(core.tick(), Ok(()));
}

#[test]
fn restart_during_a_pending_change_reverts_on_start() {
    let kit = Kit::new("confirm-restart");
    let mut core = kit.open_with_panel();
    core.set(&user(), SCALE, Value::Int(150)).unwrap();
    core.confirm(None).unwrap();
    core.set(&user(), SCALE, Value::Int(300)).unwrap();
    core.set(&user(), MODE, Value::text("mirror")).unwrap();
    core.set(&admin(), "privacy.network", Value::Bool(false))
        .unwrap();
    core.flush().unwrap();
    // The service dies here: no confirm, no revert, no tick.
    drop(core);
    kit.applier.clear_calls();

    let mut core = kit.open();
    assert!(core.pending_keys().is_empty());
    assert_eq!(core.get(SCALE), Ok(Value::Int(150)));
    assert_eq!(core.get(MODE), Ok(Value::text("extend")));
    assert_eq!(core.get("privacy.network"), Ok(Value::Bool(true)));
    // The old values were put into effect again and announced.
    assert_eq!(kit.applier.last(SCALE), Some(Value::Int(150)));
    assert_eq!(kit.applier.last(MODE), Some(Value::text("extend")));
    assert_eq!(kit.applier.last("privacy.network"), Some(Value::Bool(true)));
    let events = core.take_events();
    assert_eq!(events.len(), 6);
    assert!(events.contains(&resolved(
        SCALE,
        PendingOutcome::RevertedAtStart,
        Value::Int(150)
    )));
    assert!(events.contains(&changed("privacy.network", Value::Bool(true))));
    // And only once: the next start has nothing left to take back.
    drop(core);
    kit.applier.clear_calls();
    let mut core = kit.open();
    assert!(core.take_events().is_empty());
    assert!(kit.applier.calls().is_empty());
}

#[test]
fn restart_right_after_a_confirm_keeps_the_confirmed_value() {
    let kit = Kit::new("confirm-restart-kept");
    let mut core = kit.open_with_panel();
    core.set(&user(), MODE, Value::text("mirror")).unwrap();
    core.confirm(None).unwrap();
    core.flush().unwrap();
    drop(core);
    let mut core = kit.open();
    assert_eq!(core.get(MODE), Ok(Value::text("mirror")));
    assert!(core.take_events().is_empty());
}

#[test]
fn a_failing_applier_at_start_does_not_keep_the_service_down() {
    let kit = Kit::new("confirm-restart-fail");
    let mut core = kit.open_with_panel();
    core.set(&user(), MODE, Value::text("mirror")).unwrap();
    drop(core);
    kit.applier.fail_next(ApplyError::Failed);
    let core = kit.open();
    assert_eq!(core.stats().apply_failures, 1);
    assert_eq!(core.get(MODE), Ok(Value::text("extend")));
}

#[test]
fn setting_the_waiting_value_again_gives_the_full_time_once_more() {
    let kit = Kit::new("confirm-more");
    let mut core = kit.open_with_panel();
    core.set(&user(), MODE, Value::text("mirror")).unwrap();
    core.take_events();
    kit.advance(10);
    core.tick().unwrap();
    assert_eq!(core.set(&user(), MODE, Value::text("mirror")), Ok(true));
    assert_eq!(
        core.take_events(),
        [pending(MODE, Value::text("mirror"), Value::text("extend"))]
    );
    kit.advance(14);
    core.tick().unwrap();
    assert_eq!(
        core.get(MODE),
        Ok(Value::text("mirror")),
        "24 s after the change"
    );
    assert!(core.take_events().is_empty());
    kit.advance(1);
    core.tick().unwrap();
    assert_eq!(core.get(MODE), Ok(Value::text("extend")));
    // Without anything waiting, the same value is no change at all.
    assert_eq!(core.set(&user(), MODE, Value::text("extend")), Ok(false));
}

/// A "Keep" that arrives after the deadline – before the tick ran – does not keep the change.
#[test]
fn a_late_confirm_keeps_nothing() {
    let kit = Kit::new("confirm-late");
    let mut core = kit.open_with_panel();
    core.set(&user(), MODE, Value::text("mirror")).unwrap();
    core.take_events();
    kit.advance(16);
    assert_eq!(core.confirm(Some(MODE)), Err(SettingsError::NotPending));
    assert_eq!(core.get(MODE), Ok(Value::text("extend")));
    assert!(core.pending_keys().is_empty());
}
