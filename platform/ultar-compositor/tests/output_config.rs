//! Output management of the display service: details with the display's
//! identity, whole setups, provisional setups that go back by themselves
//! (deadline, client gone, displays changed), refusals, display numbers.

mod common;

use common::{test_config, Peer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use ultar_compositor::backend::{Backend, BackendOutput, Hotplug};
use ultar_compositor::geom::Rect as LRect;
use ultar_compositor::pixels::Image;
use ultar_compositor::{Clock, Compositor, Config, RightsTable};
use ultar_hal::{HalError, InputEvent};
use ultar_surface::proto::{
    Arrangement, ErrorCode, Event, OutputId, OutputIdentity, OutputMode, OutputSetup, Request,
    Rights, Setup, SetupOutcome, SetupRefusal, Transform,
};

const UID_DISPLAYD: u32 = 817;
const UID_APP: u32 = 20_001;

#[derive(Default)]
struct Bench {
    outputs: Vec<BackendOutput>,
    plugs: Vec<Hotplug>,
    refuse: Vec<OutputMode>,
    switches: Vec<(u32, OutputMode)>,
}

#[derive(Clone)]
struct Shared(Arc<Mutex<Bench>>);

impl Backend for Shared {
    fn outputs(&self) -> Vec<BackendOutput> {
        self.0.lock().unwrap().outputs.clone()
    }
    fn present(&mut self, _: u32, _: &Image, _: &[LRect]) -> Result<(), HalError> {
        Ok(())
    }
    fn input_fds(&self) -> Vec<std::os::fd::BorrowedFd<'_>> {
        Vec::new()
    }
    fn read_input(&mut self) -> Vec<InputEvent> {
        Vec::new()
    }
    fn hotplug(&mut self) -> Vec<Hotplug> {
        std::mem::take(&mut self.0.lock().unwrap().plugs)
    }
    fn set_mode(&mut self, hal: u32, mode: OutputMode) -> Result<(), HalError> {
        let mut b = self.0.lock().unwrap();
        if b.refuse.contains(&mode) {
            return Err(HalError::NotSupported);
        }
        let o = b.outputs.iter_mut().find(|o| o.hal == hal).unwrap();
        (o.width, o.height, o.refresh_millihertz) = (mode.width, mode.height, mode.refresh_mhz);
        b.switches.push((hal, mode));
        Ok(())
    }
}

struct TestClock(Arc<AtomicU64>);

impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn mode(w: u32, h: u32) -> OutputMode {
    OutputMode {
        width: w,
        height: h,
        refresh_mhz: 60_000,
    }
}

fn output(hal: u32, internal: bool) -> BackendOutput {
    BackendOutput {
        hal,
        name: if internal {
            "eDP-1".into()
        } else {
            "HDMI-A-1".into()
        },
        width: 1280,
        height: 800,
        refresh_millihertz: 60_000,
        width_mm: 300,
        height_mm: 190,
        internal,
        identity: OutputIdentity {
            manufacturer: if internal { "BOE".into() } else { "DEL".into() },
            product: 0x10 + hal as u16,
            serial: 7,
            name: if internal {
                String::new()
            } else {
                "DELL U2422H".into()
            },
        },
        modes: vec![mode(1280, 800), mode(1024, 768), mode(800, 600)],
    }
}

struct Rig {
    comp: Compositor,
    bench: Arc<Mutex<Bench>>,
    clock: Arc<AtomicU64>,
}

fn rig(outputs: usize) -> Rig {
    let bench = Arc::new(Mutex::new(Bench {
        outputs: (0..outputs).map(|i| output(i as u32, i == 0)).collect(),
        ..Bench::default()
    }));
    let clock = Arc::new(AtomicU64::new(1_000));
    let mut rights = RightsTable::default();
    rights.grant(UID_DISPLAYD, Rights::OUTPUT_CONFIG);
    let config = Config {
        rights,
        ..test_config()
    };
    let comp = Compositor::new(
        config,
        Box::new(Shared(bench.clone())),
        Box::new(TestClock(clock.clone())),
    );
    Rig { comp, bench, clock }
}

fn ids(comp: &Compositor) -> Vec<OutputId> {
    let mut v: Vec<OutputId> = comp.state.layout.outputs().iter().map(|o| o.id).collect();
    v.sort();
    v
}

/// Extend: output 0 primary at the origin, the others to its right.
fn extend(comp: &Compositor, serial: u32, provisional_ms: u32, m: OutputMode) -> Setup {
    let outputs = ids(comp)
        .into_iter()
        .enumerate()
        .map(|(i, id)| OutputSetup {
            output: id,
            included: true,
            primary: i == 0,
            mode: m,
            scale_milli: 1000,
            position: (i as i32 * m.width as i32, 0),
            transform: Transform::Normal,
        })
        .collect();
    Setup {
        serial,
        arrangement: Arrangement::Extend,
        provisional_ms,
        outputs,
    }
}

fn done(p: &mut Peer) -> Vec<(u32, SetupOutcome)> {
    p.take()
        .into_iter()
        .filter_map(|e| match e {
            Event::OutputSetupDone { serial, outcome } => Some((serial, outcome)),
            _ => None,
        })
        .collect()
}

fn widths(comp: &Compositor) -> Vec<u32> {
    let mut v: Vec<(OutputId, u32)> = comp
        .state
        .layout
        .outputs()
        .iter()
        .map(|o| (o.id, o.width))
        .collect();
    v.sort();
    v.into_iter().map(|(_, w)| w).collect()
}

#[test]
fn only_the_display_service_learns_details_and_may_set_outputs() {
    let mut r = rig(2);
    let service = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    let details: Vec<_> = service
        .events
        .iter()
        .filter_map(|e| match e {
            Event::OutputDetails(d) => Some(d.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(details.len(), 2);
    let external = details.iter().find(|d| !d.internal).unwrap();
    assert_eq!(external.identity.manufacturer, "DEL");
    assert_eq!(external.identity.name, "DELL U2422H");
    assert_eq!(external.modes.len(), 3);
    assert_eq!(external.size_mm, (300, 190));
    let mut app = Peer::hello_as(&mut r.comp, UID_APP);
    assert!(!app
        .events
        .iter()
        .any(|e| matches!(e, Event::OutputDetails(_))));
    let setup = extend(&r.comp, 1, 0, mode(1024, 768));
    app.send(&Request::ConfigureOutputs { setup });
    app.pump(&mut r.comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    assert_eq!(widths(&r.comp), vec![1280, 1280], "nothing changed");
    let mut other = Peer::hello_as(&mut r.comp, UID_APP);
    other.send(&Request::IdentifyOutputs {
        numbers: vec![(OutputId(0), 1)],
        ms: 5000,
    });
    other.pump(&mut r.comp);
    assert_eq!(other.error(), Some(ErrorCode::Permission));
    assert!(r.comp.state.session.manage.identify.is_none());
}

#[test]
fn a_final_setup_switches_modes_and_arranges() {
    let mut r = rig(2);
    let mut s = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    s.take();
    let mut setup = extend(&r.comp, 5, 0, mode(1024, 768));
    setup.outputs[1].primary = true;
    setup.outputs[0].primary = false;
    setup.outputs[1].scale_milli = 1250;
    s.send(&Request::ConfigureOutputs { setup });
    s.pump(&mut r.comp);
    assert_eq!(done(&mut s), vec![(5, SetupOutcome::Applied)]);
    assert_eq!(widths(&r.comp), vec![1024, 1024]);
    let primary = r.comp.state.layout.primary().unwrap();
    assert_eq!(primary.id, OutputId(1));
    assert_eq!(primary.scale_milli, 1250);
    assert_eq!(r.bench.lock().unwrap().switches.len(), 2);
    r.clock.fetch_add(60_000, Ordering::SeqCst);
    s.pump(&mut r.comp);
    assert_eq!(widths(&r.comp), vec![1024, 1024], "a final setup stays");
}

#[test]
fn a_provisional_setup_goes_back_after_its_deadline() {
    let mut r = rig(2);
    let mut s = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    s.send(&Request::ConfigureOutputs {
        setup: extend(&r.comp, 7, 15_000, mode(800, 600)),
    });
    s.pump(&mut r.comp);
    assert_eq!(done(&mut s), vec![(7, SetupOutcome::Applied)]);
    assert_eq!(widths(&r.comp), vec![800, 800]);
    r.clock.fetch_add(14_999, Ordering::SeqCst);
    s.pump(&mut r.comp);
    assert_eq!(widths(&r.comp), vec![800, 800], "not yet");
    r.clock.fetch_add(1, Ordering::SeqCst);
    s.pump(&mut r.comp);
    assert_eq!(done(&mut s), vec![(7, SetupOutcome::RevertedTimeout)]);
    assert_eq!(widths(&r.comp), vec![1280, 1280]);
    let backend: Vec<u32> = r
        .bench
        .lock()
        .unwrap()
        .outputs
        .iter()
        .map(|o| o.width)
        .collect();
    assert_eq!(backend, vec![1280, 1280], "the hardware went back too");
    assert!(!s.closed);
}

#[test]
fn a_confirmed_setup_stays_and_a_second_change_keeps_the_first_before() {
    let mut r = rig(1);
    let mut s = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    s.send(&Request::ConfigureOutputs {
        setup: extend(&r.comp, 1, 15_000, mode(1024, 768)),
    });
    s.send(&Request::ConfigureOutputs {
        setup: extend(&r.comp, 2, 15_000, mode(800, 600)),
    });
    s.pump(&mut r.comp);
    s.send(&Request::ConfirmOutputs { serial: 1 });
    s.pump(&mut r.comp);
    assert_eq!(
        done(&mut s),
        vec![
            (1, SetupOutcome::Applied),
            (2, SetupOutcome::Applied),
            (1, SetupOutcome::Refused(SetupRefusal::Invalid))
        ]
    );
    r.clock.fetch_add(15_000, Ordering::SeqCst);
    s.pump(&mut r.comp);
    assert_eq!(done(&mut s), vec![(2, SetupOutcome::RevertedTimeout)]);
    assert_eq!(widths(&r.comp), vec![1280], "back to before the first");
    s.send(&Request::ConfigureOutputs {
        setup: extend(&r.comp, 3, 15_000, mode(1024, 768)),
    });
    s.pump(&mut r.comp);
    s.send(&Request::ConfirmOutputs { serial: 3 });
    s.pump(&mut r.comp);
    assert_eq!(
        done(&mut s),
        vec![(3, SetupOutcome::Applied), (3, SetupOutcome::Confirmed)]
    );
    r.clock.fetch_add(60_000, Ordering::SeqCst);
    s.pump(&mut r.comp);
    assert_eq!(widths(&r.comp), vec![1024]);
}

#[test]
fn the_setup_goes_back_at_once_when_the_display_service_ends() {
    let mut r = rig(2);
    let mut s = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    s.send(&Request::ConfigureOutputs {
        setup: extend(&r.comp, 1, 15_000, mode(800, 600)),
    });
    s.pump(&mut r.comp);
    assert_eq!(widths(&r.comp), vec![800, 800]);
    drop(s);
    let mut other = Peer::hello_as(&mut r.comp, UID_APP);
    other.pump(&mut r.comp);
    assert_eq!(widths(&r.comp), vec![1280, 1280]);
    assert!(r.comp.state.session.manage.provisional.is_none());
}

#[test]
fn a_second_service_is_busy_while_a_setup_waits() {
    let mut r = rig(1);
    let mut a = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    let mut b = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    a.send(&Request::ConfigureOutputs {
        setup: extend(&r.comp, 1, 15_000, mode(800, 600)),
    });
    a.pump(&mut r.comp);
    b.take();
    b.send(&Request::ConfigureOutputs {
        setup: extend(&r.comp, 9, 0, mode(1024, 768)),
    });
    b.pump(&mut r.comp);
    assert_eq!(
        done(&mut b),
        vec![(9, SetupOutcome::Refused(SetupRefusal::Busy))]
    );
    b.send(&Request::ConfirmOutputs { serial: 1 });
    b.pump(&mut r.comp);
    assert_eq!(
        done(&mut b),
        vec![(1, SetupOutcome::Refused(SetupRefusal::Invalid))]
    );
    assert!(
        r.comp.state.session.manage.provisional.is_some(),
        "b cannot confirm a's setup"
    );
}

fn wrong_cases(good: Setup) -> Vec<(Setup, SetupRefusal)> {
    let mut cases: Vec<(Setup, SetupRefusal)> = Vec::new();
    let mut x = good.clone();
    x.outputs[0].mode = mode(4096, 2160);
    cases.push((x, SetupRefusal::UnknownMode));
    let mut x = good.clone();
    x.outputs.pop();
    cases.push((x, SetupRefusal::UnknownOutput));
    let mut x = good.clone();
    x.outputs[1].output = x.outputs[0].output;
    cases.push((x, SetupRefusal::UnknownOutput));
    let mut x = good.clone();
    x.outputs[1].primary = true;
    cases.push((x, SetupRefusal::Invalid));
    let mut x = good.clone();
    x.outputs[0].included = false;
    cases.push((x, SetupRefusal::Invalid));
    let mut x = good.clone();
    x.outputs[1].position = (10, 10);
    cases.push((x, SetupRefusal::Invalid));
    let mut x = good.clone();
    x.outputs[1].scale_milli = 1100;
    cases.push((x, SetupRefusal::Invalid));
    let mut x = good.clone();
    x.arrangement = Arrangement::Single;
    cases.push((x, SetupRefusal::Invalid));
    let mut x = good.clone();
    x.outputs[0].transform = Transform::Rotate90;
    cases.push((x, SetupRefusal::NotSupported));
    cases
}

#[test]
fn wrong_setups_change_nothing() {
    let mut r = rig(2);
    let mut s = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    let cases = wrong_cases(extend(&r.comp, 1, 0, mode(1024, 768)));
    for (i, (mut setup, refusal)) in cases.into_iter().enumerate() {
        setup.serial = 100 + i as u32;
        s.take();
        s.send(&Request::ConfigureOutputs { setup });
        s.pump(&mut r.comp);
        assert_eq!(
            done(&mut s),
            vec![(100 + i as u32, SetupOutcome::Refused(refusal))],
            "case {i}"
        );
        assert_eq!(widths(&r.comp), vec![1280, 1280], "case {i}");
    }
    // The hardware refuses the second output's switch: the first goes back.
    r.bench.lock().unwrap().refuse.push(mode(800, 600));
    let mut setup = extend(&r.comp, 50, 0, mode(1024, 768));
    setup.outputs[1].mode = mode(800, 600);
    s.send(&Request::ConfigureOutputs { setup });
    s.pump(&mut r.comp);
    assert_eq!(
        done(&mut s),
        vec![(50, SetupOutcome::Refused(SetupRefusal::NotSupported))]
    );
    let backend: Vec<u32> = r
        .bench
        .lock()
        .unwrap()
        .outputs
        .iter()
        .map(|o| o.width)
        .collect();
    assert_eq!(backend, vec![1280, 1280]);
    assert!(!s.closed, "refusals are answers, not protocol errors");
}

#[test]
fn single_and_mirror_arrangements() {
    let mut r = rig(2);
    let mut s = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    let mut setup = extend(&r.comp, 1, 0, mode(1280, 800));
    setup.arrangement = Arrangement::Single;
    setup.outputs[1].included = false;
    s.send(&Request::ConfigureOutputs {
        setup: setup.clone(),
    });
    s.pump(&mut r.comp);
    assert_eq!(r.comp.state.layout.enabled().count(), 1);
    setup.serial = 2;
    setup.arrangement = Arrangement::Mirror;
    setup.outputs[1].included = true;
    s.send(&Request::ConfigureOutputs { setup });
    s.pump(&mut r.comp);
    assert_eq!(r.comp.state.layout.enabled().count(), 2);
    assert!(r.comp.state.layout.enabled().all(|o| o.x == 0 && o.y == 0));
}

#[test]
fn displays_changing_take_a_waiting_setup_back() {
    let mut r = rig(1);
    let mut s = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    s.send(&Request::ConfigureOutputs {
        setup: extend(&r.comp, 4, 15_000, mode(800, 600)),
    });
    s.pump(&mut r.comp);
    s.take();
    {
        let mut b = r.bench.lock().unwrap();
        let o = output(1, false);
        b.outputs.push(o.clone());
        b.plugs.push(Hotplug::Added(o));
    }
    r.comp.rescan_outputs();
    s.pump(&mut r.comp);
    let events = s.take();
    assert!(events.iter().any(|e| matches!(
        e,
        Event::OutputSetupDone {
            serial: 4,
            outcome: SetupOutcome::RevertedHotplug
        }
    )));
    assert_eq!(widths(&r.comp), vec![1280, 1280]);
    let details = events
        .iter()
        .filter(|e| matches!(e, Event::OutputDetails(_)))
        .count();
    assert!(details >= 2, "the service learns the new display");
    s.send(&Request::DetectOutputs);
    s.pump(&mut r.comp);
    assert!(s
        .take()
        .iter()
        .any(|e| matches!(e, Event::OutputsDetected { count: 2 })));
}

#[test]
fn display_numbers_show_for_their_time_and_compose_like_a_full_frame() {
    let mut r = rig(2);
    let mut s = Peer::hello_as(&mut r.comp, UID_DISPLAYD);
    r.comp.render_now();
    let id = ids(&r.comp)[1];
    let before = r.comp.frame(id).unwrap().data().to_vec();
    s.send(&Request::IdentifyOutputs {
        numbers: vec![(ids(&r.comp)[0], 1), (id, 2), (OutputId(99), 3)],
        ms: 5000,
    });
    s.pump(&mut r.comp);
    r.comp.render_now();
    let shown = r.comp.frame(id).unwrap().data().to_vec();
    assert_ne!(before, shown, "the number is drawn");
    let reference = r.comp.render_reference(id).unwrap();
    assert_eq!(reference.data(), &shown[..], "incremental == full");
    assert_eq!(
        r.comp
            .state
            .session
            .manage
            .identify
            .as_ref()
            .unwrap()
            .numbers
            .len(),
        2
    );
    r.clock.fetch_add(5000, Ordering::SeqCst);
    s.pump(&mut r.comp);
    r.comp.render_now();
    assert!(r.comp.state.session.manage.identify.is_none());
    assert_eq!(r.comp.frame(id).unwrap().data(), &before[..], "gone again");
}
