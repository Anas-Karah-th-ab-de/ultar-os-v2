use super::*;
use ultar_surface::proto::{OutputId, OutputIdentity};

fn m(w: u32, h: u32, r: u32) -> OutputMode {
    OutputMode {
        width: w,
        height: h,
        refresh_mhz: r,
    }
}

fn display(i: u32, internal: bool) -> Connected {
    Connected {
        details: OutputDetails {
            id: OutputId(i),
            connector: format!("c-{i}"),
            identity: OutputIdentity::default(),
            internal,
            size_mm: (0, 0),
            preferred: m(1920, 1080, 60_000),
            current: m(1920, 1080, 60_000),
            modes: vec![
                m(1920, 1080, 60_000),
                m(1920, 1080, 144_000),
                m(1280, 720, 50_000),
                m(1280, 720, 60_000),
            ],
        },
        id: format!("d{i}"),
    }
}

fn values() -> DisplayValues {
    DisplayValues::default()
}

#[test]
fn modes_follow_resolution_and_rate() {
    let d = display(0, true).details;
    let mut per = PerDisplay::default();
    assert_eq!(pick_mode(&d, &per), m(1920, 1080, 60_000));
    per.refresh_hz = 143.9;
    assert_eq!(pick_mode(&d, &per), m(1920, 1080, 144_000));
    per.resolution = Some((1280, 720));
    per.refresh_hz = 0.0;
    assert_eq!(pick_mode(&d, &per), m(1280, 720, 60_000), "highest rate");
    per.refresh_hz = 50.0;
    assert_eq!(pick_mode(&d, &per), m(1280, 720, 50_000));
    per.resolution = Some((640, 480));
    assert_eq!(pick_mode(&d, &per), d.preferred, "unknown resolution");
}

#[test]
fn extend_places_overlapping_displays_side_by_side_from_the_main_one() {
    let connected = [display(0, false), display(1, true)];
    let setup = plan(&connected, &values(), 3, 0);
    assert_eq!(setup.arrangement, Arrangement::Extend);
    assert!(setup.outputs[1].primary, "the built-in display is main");
    assert_eq!(setup.outputs[1].position, (0, 0));
    assert_eq!(setup.outputs[0].position, (1920, 0));
    let mut v = values();
    v.primary = "d0".into();
    v.displays.insert(
        "d1".into(),
        PerDisplay {
            scale: 200,
            position: (-960, 0),
            ..PerDisplay::default()
        },
    );
    let setup = plan(&connected, &v, 4, 18_000);
    assert!(setup.outputs[0].primary);
    assert_eq!(
        setup.outputs[1].position,
        (-960, 0),
        "stored positions stay"
    );
    assert_eq!(setup.outputs[1].scale_milli, 2000);
    assert_eq!(setup.provisional_ms, 18_000);
}

#[test]
fn single_mirror_and_one_display() {
    let connected = [display(0, true), display(1, false), display(2, false)];
    let mut v = values();
    v.mode = ModeWord::Single;
    v.single = "d2".into();
    let s = plan(&connected, &v, 1, 0);
    assert_eq!(s.arrangement, Arrangement::Single);
    assert_eq!(
        s.outputs
            .iter()
            .map(|o| (o.included, o.primary))
            .collect::<Vec<_>>(),
        vec![(false, false), (false, false), (true, true)]
    );
    v.single = "gone".into();
    let s = plan(&connected, &v, 1, 0);
    assert!(
        s.outputs[0].included && s.outputs[0].primary,
        "falls back to the main display"
    );
    v.mode = ModeWord::Mirror;
    v.mirror = vec!["d1".into(), "d2".into()];
    let s = plan(&connected, &v, 1, 0);
    assert_eq!(s.arrangement, Arrangement::Mirror);
    assert_eq!(
        s.outputs
            .iter()
            .map(|o| (o.included, o.primary))
            .collect::<Vec<_>>(),
        vec![(false, false), (true, true), (true, false)]
    );
    v.mirror.clear();
    assert!(plan(&connected, &v, 1, 0)
        .outputs
        .iter()
        .all(|o| o.included));
    let one = plan(&connected[..1], &v, 1, 0);
    assert_eq!(one.arrangement, Arrangement::Extend);
    assert!(one.outputs[0].primary && one.outputs[0].included);
}

#[test]
fn turned_displays_are_shown_upright_and_reported() {
    let connected = [display(0, true)];
    let mut v = values();
    v.displays.insert(
        "d0".into(),
        PerDisplay {
            transform: Transform::Rotate90,
            scale: 113,
            ..PerDisplay::default()
        },
    );
    let mut s = plan(&connected, &v, 1, 0);
    assert_eq!(s.outputs[0].scale_milli, 1250, "25 % steps");
    assert!(upright(&mut s));
    assert!(!upright(&mut s));
    let mut upright_values = v.clone();
    if let Some(d) = upright_values.displays.get_mut("d0") {
        d.transform = Transform::Normal;
    }
    let t = plan(&connected, &upright_values, 2, 15_000);
    assert!(same_arrangement(&s, &t));
    assert!(main_display(&[], &v).is_none());
}
