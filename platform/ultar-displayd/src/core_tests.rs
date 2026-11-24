use super::*;
use crate::ports::Item;
use crate::service;
use std::cell::RefCell;
use ultar_bus::messages::display_service::{code, Apply, Change, Keep, Projection};
use ultar_bus::messages::settings::DisplayDescriptor;
use ultar_surface::proto::{OutputIdentity, OutputMode, SetupRefusal};

#[derive(Default)]
struct FakeSettings {
    values: RefCell<BTreeMap<String, SettingValue>>,
    pending: RefCell<Vec<String>>,
    calls: RefCell<Vec<String>>,
    down: RefCell<bool>,
    refuse_key: RefCell<Option<String>>,
}

impl SettingsPort for FakeSettings {
    fn set_displays(&self, d: &[DisplayDescriptor]) -> Result<Vec<String>, PortError> {
        if *self.down.borrow() {
            return Err(PortError::Unavailable);
        }
        self.calls
            .borrow_mut()
            .push(format!("set_displays {}", d.len()));
        Ok(d.iter().map(|x| format!("d-{}", x.connector)).collect())
    }
    fn display_items(&self) -> Result<Vec<Item>, PortError> {
        let pending = self.pending.borrow();
        Ok(self
            .values
            .borrow()
            .iter()
            .map(|(k, v)| Item {
                key: k.clone(),
                value: v.clone(),
                pending: pending.contains(k),
            })
            .collect())
    }
    fn set(&self, key: &str, value: SettingValue) -> Result<(), PortError> {
        if self.refuse_key.borrow().as_deref() == Some(key) {
            return Err(PortError::Refused(3));
        }
        self.calls.borrow_mut().push(format!("set {key}"));
        self.values.borrow_mut().insert(key.into(), value);
        Ok(())
    }
    fn confirm(&self, key: &str) -> Result<u32, PortError> {
        self.calls.borrow_mut().push(format!("confirm {key}"));
        Ok(1)
    }
    fn revert(&self, key: &str) -> Result<u32, PortError> {
        self.calls.borrow_mut().push(format!("revert {key}"));
        self.pending.borrow_mut().retain(|k| k != key);
        Ok(1)
    }
}

#[derive(Default)]
struct FakeComp(RefCell<Vec<Request>>);

impl CompositorPort for FakeComp {
    fn send(&self, r: Request) -> bool {
        self.0.borrow_mut().push(r);
        true
    }
}

#[derive(Default)]
struct FakePub(RefCell<Vec<Outputs>>);

impl Publisher for FakePub {
    fn outputs(&self, o: &Outputs) {
        self.0.borrow_mut().push(o.clone());
    }
}

#[derive(Default)]
struct Rig {
    settings: FakeSettings,
    comp: FakeComp,
    publish: FakePub,
}

impl Rig {
    fn new() -> (Rig, Core) {
        (Rig::default(), Core::new())
    }
    fn settle(&self, core: &mut Core, now: u64) {
        core.settle(now, &self.ports());
    }
    fn sent(&self) -> Vec<Request> {
        self.comp.0.borrow_mut().drain(..).collect()
    }
    fn calls(&self) -> Vec<String> {
        self.settings.calls.borrow_mut().drain(..).collect()
    }
    fn ports(&self) -> Ports<'_> {
        Ports {
            settings: &self.settings,
            compositor: &self.comp,
            publish: &self.publish,
        }
    }
}

fn details(i: u32, internal: bool) -> OutputDetails {
    let m = OutputMode {
        width: 1280,
        height: 800,
        refresh_mhz: 60_000,
    };
    OutputDetails {
        id: OutputId(i),
        connector: format!("c{i}"),
        identity: OutputIdentity {
            manufacturer: "UHL".into(),
            product: 1,
            serial: i + 1,
            name: "H".into(),
        },
        internal,
        size_mm: (0, 0),
        preferred: m,
        current: m,
        modes: vec![m],
    }
}

fn setup_of(r: &Request) -> Option<&Setup> {
    match r {
        Request::ConfigureOutputs { setup } => Some(setup),
        _ => None,
    }
}

fn two_displays() -> (Rig, Core) {
    let (r, mut core) = Rig::new();
    core.on_details(details(0, true));
    core.on_details(details(1, false));
    r.settle(&mut core, 0);
    (r, core)
}

#[test]
fn first_settle_reports_displays_publishes_and_sends_a_final_setup() {
    let (r, mut core) = two_displays();
    assert_eq!(r.calls(), vec!["set_displays 2"]);
    let sent = r.sent();
    assert_eq!(sent.len(), 1);
    let s = setup_of(&sent[0]).unwrap();
    assert_eq!(s.provisional_ms, 0);
    assert_eq!(r.publish.0.borrow().len(), 1);
    assert_eq!(r.publish.0.borrow()[0].outputs[1].id, "d-c1");
    r.settle(&mut core, 10);
    assert!(r.sent().is_empty(), "nothing changed, nothing sent");
    assert_eq!(r.publish.0.borrow().len(), 1);
}

#[test]
fn a_waiting_change_is_provisional_and_kept_by_confirming() {
    let (r, mut core) = two_displays();
    r.sent();
    core.on_changed("display.mode", SettingValue::Text("single".into()));
    core.on_pending("display.mode", 15, 1_000);
    r.settle(&mut core, 1_000);
    let sent = r.sent();
    let s = setup_of(&sent[0]).unwrap();
    assert_eq!(s.provisional_ms, 15_000 + GRACE_MS as u32);
    assert_eq!(s.outputs.iter().filter(|o| o.included).count(), 1);
    let serial = s.serial;
    core.on_resolved("display.mode");
    r.settle(&mut core, 5_000);
    assert_eq!(r.sent(), vec![Request::ConfirmOutputs { serial }]);
    r.settle(&mut core, 6_000);
    assert!(r.sent().is_empty());
}

#[test]
fn a_reverted_change_sends_the_old_arrangement_as_final() {
    let (r, mut core) = two_displays();
    r.sent();
    core.on_changed("display.mode", SettingValue::Text("mirror".into()));
    core.on_pending("display.mode", 15, 0);
    r.settle(&mut core, 0);
    assert_eq!(
        setup_of(&r.sent()[0]).unwrap().arrangement,
        ultar_surface::proto::Arrangement::Mirror
    );
    core.on_changed("display.mode", SettingValue::Text("extend".into()));
    core.on_resolved("display.mode");
    r.settle(&mut core, 15_000);
    let sent = r.sent();
    let s = setup_of(&sent[0]).unwrap();
    assert_eq!(s.arrangement, ultar_surface::proto::Arrangement::Extend);
    assert_eq!(s.provisional_ms, 0);
}

#[test]
fn changes_waiting_at_start_are_taken_back() {
    let (r, mut core) = Rig::new();
    r.settings
        .values
        .borrow_mut()
        .insert("display.mode".into(), SettingValue::Text("mirror".into()));
    r.settings.pending.borrow_mut().push("display.mode".into());
    core.on_details(details(0, true));
    r.settle(&mut core, 0);
    assert_eq!(r.calls(), vec!["set_displays 1", "revert display.mode"]);
    assert_eq!(core.take_notes(), vec![Note::RevertedAtStart(1)]);
}

#[test]
fn refusals_and_compositor_reverts_take_the_setting_back() {
    let (r, mut core) = two_displays();
    r.sent();
    r.calls();
    core.on_changed("display.mode", SettingValue::Text("mirror".into()));
    core.on_pending("display.mode", 15, 0);
    r.settle(&mut core, 0);
    let serial = setup_of(&r.sent()[0]).unwrap().serial;
    let outcome = SetupOutcome::Refused(SetupRefusal::NotSupported);
    core.on_setup_done(serial + 7, outcome, &r.ports());
    assert!(r.calls().is_empty(), "an old serial is ignored");
    core.on_setup_done(serial, outcome, &r.ports());
    assert_eq!(r.calls(), vec!["revert display.mode"]);
    assert!(core.pending().is_empty());
    assert_eq!(core.take_notes(), vec![Note::SetupRefused(outcome)]);
    assert!(core.held());
    // settingsd announces the value it went back to.
    core.on_changed("display.mode", SettingValue::Text("extend".into()));
    core.on_resolved("display.mode");
    r.settle(&mut core, 0);
    assert_eq!(setup_of(&r.sent()[0]).unwrap().provisional_ms, 0);
    core.on_changed("display.mode", SettingValue::Text("single".into()));
    core.on_pending("display.mode", 15, 0);
    r.settle(&mut core, 0);
    let serial = setup_of(&r.sent()[0]).unwrap().serial;
    *r.settings.down.borrow_mut() = true;
    core.on_setup_done(serial, SetupOutcome::RevertedTimeout, &r.ports());
    assert!(core.sent().is_none() && core.held());
    r.settle(&mut core, 20_000);
    assert!(
        r.sent().is_empty(),
        "the compositor's step back is not undone"
    );
    // settingsd comes back and takes the change back: the arrangement follows.
    core.on_changed("display.mode", SettingValue::Text("extend".into()));
    core.on_resolved("display.mode");
    r.settle(&mut core, 21_000);
    let sent = r.sent();
    assert_eq!(setup_of(&sent[0]).unwrap().provisional_ms, 0);
}

#[test]
fn an_unavailable_settings_service_is_asked_again_later() {
    let (r, mut core) = Rig::new();
    *r.settings.down.borrow_mut() = true;
    core.on_details(details(0, true));
    r.settle(&mut core, 0);
    assert_eq!(core.next_deadline(), Some(RETRY_MS));
    assert_eq!(core.take_notes(), vec![Note::SettingsUnavailable]);
    assert!(r.sent().is_empty());
    *r.settings.down.borrow_mut() = false;
    r.settle(&mut core, RETRY_MS - 1);
    assert!(r.calls().is_empty(), "not before the pause");
    r.settle(&mut core, RETRY_MS);
    assert_eq!(r.calls(), vec!["set_displays 1"]);
    assert_eq!(r.sent().len(), 1);
}

#[test]
fn hotplug_and_a_lost_compositor() {
    let (r, mut core) = two_displays();
    r.calls();
    r.sent();
    core.on_removed(OutputId(1));
    r.settle(&mut core, 5);
    assert_eq!(r.calls(), vec!["set_displays 1"]);
    assert_eq!(setup_of(&r.sent()[0]).unwrap().outputs.len(), 1);
    core.on_compositor_gone();
    r.settle(&mut core, 6);
    assert!(r.sent().is_empty(), "no displays, nothing to send");
    core.on_details(details(0, true));
    r.settle(&mut core, 7);
    assert_eq!(
        r.sent().len(),
        1,
        "sent again after the compositor came back"
    );
    core.resync();
    r.settle(&mut core, 8);
    assert_eq!(r.calls(), vec!["set_displays 1", "set_displays 1"]);
    assert_eq!(core.numbers(&[]), vec![(OutputId(0), 1)]);
    assert_eq!(
        core.numbers(&["d-c0".into(), "x".into()]),
        vec![(OutputId(0), 1)]
    );
}

#[test]
fn apply_keep_more_and_revert() {
    let (r, mut core) = two_displays();
    r.calls();
    r.sent();
    let apply = Apply {
        change: Change::Projection(Projection::Extend),
    };
    let reply = service::apply(&mut core, &r.ports(), &apply).unwrap();
    assert_eq!(reply.seconds, KEEP_SECONDS);
    assert_eq!(r.calls(), vec!["set display.mode"]);
    core.on_changed("display.mode", SettingValue::Text("extend".into()));
    let again = service::apply(&mut core, &r.ports(), &apply).unwrap();
    assert_eq!((again.seconds, again.pending.len()), (0, 0), "already so");
    assert_eq!(
        service::keep(&mut core, &r.ports(), Keep { more: false }),
        Err(code::NOT_PENDING)
    );
    let only = Apply {
        change: Change::Projection(Projection::ThisDeviceOnly),
    };
    *r.settings.refuse_key.borrow_mut() = Some("display.single_display".into());
    assert_eq!(
        service::apply(&mut core, &r.ports(), &only),
        Err(code::INVALID)
    );
    assert_eq!(
        r.calls(),
        vec!["set display.mode", "revert display.mode"],
        "all or nothing"
    );
    *r.settings.refuse_key.borrow_mut() = None;
    service::apply(&mut core, &r.ports(), &only).unwrap();
    r.calls();
    core.on_changed("display.mode", SettingValue::Text("single".into()));
    core.on_pending("display.mode", 15, 0);
    r.settle(&mut core, 0);
    r.sent();
    let more = service::keep(&mut core, &r.ports(), Keep { more: true }).unwrap();
    assert_eq!(more.resolved, 1);
    assert_eq!(r.calls(), vec!["set display.mode"]);
    core.on_pending("display.mode", 15, 10_000);
    r.settle(&mut core, 10_000);
    let resent = r.sent();
    assert_eq!(
        setup_of(&resent[0]).unwrap().provisional_ms,
        18_000,
        "re-armed"
    );
    let kept = service::keep(&mut core, &r.ports(), Keep { more: false }).unwrap();
    assert_eq!(kept.resolved, 1);
    let back = service::revert(&mut core, &r.ports()).unwrap();
    assert_eq!(back.resolved, 1);
    assert_eq!(
        service::revert(&mut core, &r.ports()),
        Err(code::NOT_PENDING)
    );
    let id = ultar_bus::messages::display_service::Identify { order: vec![] };
    assert_eq!(service::identify(&core, &r.ports(), &id).unwrap().shown, 2);
}

#[test]
fn a_changed_setting_waits_for_its_companion_before_the_compositor_follows() {
    let (r, mut core) = two_displays();
    r.sent();
    core.on_announced("display.mode", SettingValue::Text("single".into()), 100);
    r.settle(&mut core, 100);
    assert!(r.sent().is_empty(), "not yet: is it provisional?");
    assert_eq!(core.next_deadline(), Some(100 + COMPANION_MS));
    core.on_pending("display.mode", 15, 150);
    r.settle(&mut core, 150);
    let sent = r.sent();
    assert!(
        setup_of(&sent[0]).unwrap().provisional_ms > 0,
        "sent as provisional"
    );
    // A change nobody announces as provisional (a new combination) follows
    // after the short wait.
    core.on_announced("display.primary", SettingValue::Text("d-c1".into()), 1_000);
    r.settle(&mut core, 1_000);
    assert!(r.sent().is_empty());
    core.on_resolved("display.mode");
    r.settle(&mut core, 1_000 + COMPANION_MS);
    assert_eq!(r.sent().len(), 1);
}
