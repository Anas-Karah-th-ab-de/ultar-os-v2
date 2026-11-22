//! Display-combination profiles: the dock at home is not the projector at
//! the office.

use super::super::testkit::*;
use super::super::*;
use crate::schema::ValueError;
use crate::value::Value;

const PANEL: &str = "boe-ne135";
const DOCK: &str = "del-u2720q";
const BEAMER: &str = "eps-eb-u42";

fn key(id: &str, item: &str) -> String {
    format!("display.{id}.{item}")
}

#[test]
fn first_time_gives_sensible_defaults() {
    let kit = Kit::new("combo-first");
    let mut core = kit.open();
    let reply = core.set_displays(&admin(), &[dock(), panel()]).unwrap();
    assert!(reply.first_time);
    assert_eq!(reply.ids, [DOCK, PANEL]);
    assert_eq!(reply.combination, core.combination());
    assert_eq!(core.get("display.mode"), Ok(Value::text("extend")));
    assert_eq!(core.get("display.primary"), Ok(Value::text(PANEL)));
    assert_eq!(core.get(&key(PANEL, "scale")), Ok(Value::Int(200)));
    assert_eq!(core.get(&key(DOCK, "scale")), Ok(Value::Int(100)));
    assert_eq!(
        core.get(&key(PANEL, "resolution")),
        Ok(Value::text("2880x1800"))
    );
    assert_eq!(core.get(&key(PANEL, "position_x")), Ok(Value::Int(0)));
    assert_eq!(core.get(&key(DOCK, "position_x")), Ok(Value::Int(1440)));
    assert_eq!(core.get(&key(DOCK, "refresh_rate")), Ok(Value::Float(60.0)));
    // Everything of the arrangement was handed to the applier.
    assert_eq!(
        kit.applier.last("display.mode"),
        Some(Value::text("extend"))
    );
    assert_eq!(
        kit.applier.last(&key(DOCK, "position_x")),
        Some(Value::Int(1440))
    );
    assert_eq!(kit.applier.calls().len(), 4 + 2 * 11);
    // The same set again is not a first time.
    assert!(
        !core
            .set_displays(&admin(), &[panel(), dock()])
            .unwrap()
            .first_time
    );
}

#[test]
fn each_combination_gets_its_own_arrangement_back() {
    let kit = Kit::new("combo-home-office");
    let mut core = kit.open();
    let who = admin();

    // At home: the monitor is the primary display, the panel is scaled down.
    let home = core
        .set_displays(&who, &[panel(), dock()])
        .unwrap()
        .combination;
    core.set(&who, "display.primary", Value::text(DOCK))
        .unwrap();
    core.set(&who, &key(PANEL, "scale"), Value::Int(175))
        .unwrap();
    core.set(&who, &key(DOCK, "brightness"), Value::Int(40))
        .unwrap();
    core.confirm(None).unwrap();

    // At the office: mirror onto the projector.
    let office = core.set_displays(&who, &[panel(), projector()]).unwrap();
    assert!(office.first_time);
    assert_ne!(office.combination, home);
    assert_eq!(core.get("display.primary"), Ok(Value::text(PANEL)));
    assert_eq!(core.get(&key(PANEL, "scale")), Ok(Value::Int(200)));
    core.set(&who, "display.mode", Value::text("mirror"))
        .unwrap();
    core.confirm(None).unwrap();

    // On the road: the panel alone.
    core.set_displays(&who, &[panel()]).unwrap();
    assert_eq!(core.get("display.mode"), Ok(Value::text("extend")));
    assert_eq!(core.get(&key(PANEL, "scale")), Ok(Value::Int(200)));
    assert_eq!(
        core.get(&key(DOCK, "brightness")),
        Err(SettingsError::UnknownInstance)
    );

    // Home again, plugged in the other order.
    core.take_events();
    kit.applier.clear_calls();
    let back = core.set_displays(&who, &[dock(), panel()]).unwrap();
    assert!(!back.first_time);
    assert_eq!(back.combination, home);
    assert_eq!(core.get("display.primary"), Ok(Value::text(DOCK)));
    assert_eq!(core.get(&key(PANEL, "scale")), Ok(Value::Int(175)));
    assert_eq!(core.get(&key(DOCK, "brightness")), Ok(Value::Int(40)));
    assert_eq!(core.get("display.mode"), Ok(Value::text("extend")));
    // The stored arrangement was applied and what differs was announced.
    assert_eq!(kit.applier.last("display.primary"), Some(Value::text(DOCK)));
    assert_eq!(
        kit.applier.last(&key(PANEL, "scale")),
        Some(Value::Int(175))
    );
    let events = core.take_events();
    assert!(events.contains(&changed("display.primary", Value::text(DOCK))));
    assert!(events.contains(&changed(&key(PANEL, "scale"), Value::Int(175))));
    assert!(events.contains(&changed(&key(DOCK, "brightness"), Value::Int(40))));
    assert!(!events.contains(&changed("display.mode", Value::text("extend"))));
}

#[test]
fn going_back_and_forth_keeps_both_arrangements() {
    let kit = Kit::new("combo-forth");
    let mut core = kit.open();
    let who = admin();
    for round in 0..3 {
        core.set_displays(&who, &[panel(), dock()]).unwrap();
        if round == 0 {
            core.set(&who, "display.mode", Value::text("single"))
                .unwrap();
            core.confirm(None).unwrap();
        }
        assert_eq!(core.get("display.mode"), Ok(Value::text("single")));
        core.set_displays(&who, &[projector(), panel()]).unwrap();
        if round == 0 {
            core.set(&who, "display.mode", Value::text("mirror"))
                .unwrap();
            core.confirm(None).unwrap();
        }
        assert_eq!(core.get("display.mode"), Ok(Value::text("mirror")));
    }
}

#[test]
fn arrangements_survive_a_restart() {
    let kit = Kit::new("combo-restart");
    let mut core = kit.open();
    core.set_displays(&admin(), &[panel(), dock()]).unwrap();
    core.set(&admin(), "display.mode", Value::text("mirror"))
        .unwrap();
    core.confirm(None).unwrap();
    core.set_displays(&admin(), &[panel()]).unwrap();
    core.flush().unwrap();
    drop(core);

    let mut core = kit.open();
    // Until the display service reports, the combination used last holds.
    assert_eq!(core.displays().len(), 1);
    assert_eq!(core.get("display.mode"), Ok(Value::text("extend")));
    let reply = core.set_displays(&admin(), &[dock(), panel()]).unwrap();
    assert!(!reply.first_time);
    assert_eq!(core.get("display.mode"), Ok(Value::text("mirror")));
}

#[test]
fn display_keys_and_values_must_name_connected_displays() {
    let kit = Kit::new("combo-unknown");
    let mut core = kit.open_with_panel();
    let who = admin();
    assert_eq!(
        core.get(&key(BEAMER, "scale")),
        Err(SettingsError::UnknownInstance)
    );
    assert_eq!(
        core.set(&who, &key(BEAMER, "scale"), Value::Int(100)),
        Err(SettingsError::UnknownInstance)
    );
    let unknown = Err(SettingsError::Value(ValueError::BadFormat {
        format: "display",
    }));
    assert_eq!(
        core.set(&who, "display.primary", Value::text(BEAMER)),
        unknown
    );
    assert_eq!(
        core.set(
            &who,
            "display.mirror_displays",
            Value::list(&[PANEL, BEAMER])
        ),
        unknown
    );
    assert_eq!(
        core.set(&who, "display.mirror_displays", Value::list(&[PANEL])),
        Ok(true)
    );
    // Empty means "choose automatically" and is always acceptable.
    core.confirm(None).unwrap();
    assert!(core.set(&who, "display.primary", Value::text("")).is_ok());
}

#[test]
fn pending_display_change_is_dropped_when_the_displays_change() {
    let kit = Kit::new("combo-pending");
    let mut core = kit.open();
    let who = admin();
    core.set_displays(&who, &[panel(), dock()]).unwrap();
    core.set(&who, "display.mode", Value::text("mirror"))
        .unwrap();
    core.set(&who, "privacy.network", Value::Bool(false))
        .unwrap();
    core.take_events();
    // The monitor is unplugged before anybody answered "keep?".
    core.set_displays(&who, &[panel()]).unwrap();
    assert_eq!(core.pending_keys(), ["privacy.network"]);
    let events = core.take_events();
    assert!(events.contains(&Event::Resolved {
        key: "display.mode".into(),
        outcome: PendingOutcome::DisplaysChanged,
        value: Value::text("extend"),
    }));
    // Back at the dock the unconfirmed mirror mode is not there.
    core.set_displays(&who, &[panel(), dock()]).unwrap();
    assert_eq!(core.get("display.mode"), Ok(Value::text("extend")));
}

#[test]
fn the_same_displays_reported_again_keep_a_pending_change() {
    let kit = Kit::new("combo-same");
    let mut core = kit.open_with_panel();
    core.set(&admin(), "display.mode", Value::text("single"))
        .unwrap();
    core.set_displays(&admin(), &[panel()]).unwrap();
    assert_eq!(core.pending_keys(), ["display.mode"]);
    assert_eq!(core.get("display.mode"), Ok(Value::text("single")));
    assert_eq!(
        kit.applier.last("display.mode"),
        Some(Value::text("single"))
    );
}

#[test]
fn too_many_displays_are_a_bad_request_and_change_nothing() {
    let kit = Kit::new("combo-many");
    let mut core = kit.open_with_panel();
    let before = core.combination().to_owned();
    let many = vec![projector(); crate::displays::MAX_DISPLAYS + 1];
    assert_eq!(
        core.set_displays(&admin(), &many).unwrap_err(),
        SettingsError::BadRequest
    );
    assert_eq!(core.combination(), before);
    // No display at all is fine (headless, lid closed without monitor).
    let none = core.set_displays(&admin(), &[]).unwrap();
    assert!(none.ids.is_empty());
    assert_eq!(core.get("display.primary"), Ok(Value::text("")));
}

#[test]
fn failing_applier_during_a_switch_is_counted_not_fatal() {
    let kit = Kit::new("combo-apply-fail");
    let mut core = kit.open();
    kit.applier
        .fail_key("display.mode", crate::apply::ApplyError::Busy);
    assert!(core.set_displays(&admin(), &[panel()]).is_ok());
    assert_eq!(core.stats().apply_failures, 1);
    assert_eq!(
        kit.applier.last("display.boe-ne135.scale"),
        Some(Value::Int(200))
    );
}
