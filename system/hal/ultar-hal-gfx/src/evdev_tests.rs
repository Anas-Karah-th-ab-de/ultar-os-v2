use super::*;
use std::io::Write;

fn record(kind: u16, code: u16, value: i32) -> [u8; RECORD] {
    let mut r = [0u8; RECORD];
    r[0..8].copy_from_slice(&1u64.to_ne_bytes());
    r[8..16].copy_from_slice(&500u64.to_ne_bytes());
    r[16..18].copy_from_slice(&kind.to_ne_bytes());
    r[18..20].copy_from_slice(&code.to_ne_bytes());
    r[20..24].copy_from_slice(&value.to_ne_bytes());
    r
}

fn decoder(multitouch: bool) -> Decoder {
    let range = AxisRange { min: 0, max: 1000 };
    Decoder::new(InputDeviceId(3), range, range, multitouch)
}

fn feed(d: &mut Decoder, records: &[(u16, u16, i32)]) -> Vec<InputEventKind> {
    let mut out = Vec::new();
    for &(k, c, v) in records {
        d.feed(&record(k, c, v), &mut out);
    }
    assert!(out
        .iter()
        .all(|e| e.device == InputDeviceId(3) && e.time_us == 1_000_500));
    out.into_iter().map(|e| e.kind).collect()
}

#[test]
fn axis_normalisation() {
    let r = AxisRange { min: 100, max: 300 };
    assert_eq!(r.normalise(100), 0);
    assert_eq!(r.normalise(300), 65535);
    assert_eq!(r.normalise(-5), 0);
    assert_eq!(r.normalise(1000), 65535);
    assert_eq!(AxisRange { min: 5, max: 5 }.normalise(5), 0);
}

#[test]
fn keys_relative_motion_wheel_and_buttons() {
    let mut d = decoder(false);
    let got = feed(
        &mut d,
        &[
            (EV_KEY, 30, 1),
            (EV_KEY, 30, 2),
            (EV_REL, REL_X, 3),
            (EV_REL, REL_X, 2),
            (EV_REL, REL_Y, -1),
            (EV_REL, REL_WHEEL, 1),
            (EV_KEY, BTN_LEFT, 1),
            (EV_KEY, BTN_RIGHT, 0),
            (EV_SYN, SYN_REPORT, 0),
        ],
    );
    assert_eq!(
        got,
        vec![
            InputEventKind::Key {
                code: 30,
                state: KeyState::Pressed
            },
            InputEventKind::Key {
                code: 30,
                state: KeyState::Repeated
            },
            InputEventKind::Scroll { dx: 0.0, dy: -1.0 },
            InputEventKind::PointerButton {
                button: PointerButton::Primary,
                pressed: true
            },
            InputEventKind::PointerButton {
                button: PointerButton::Secondary,
                pressed: false
            },
            InputEventKind::PointerMotion { dx: 5.0, dy: -1.0 },
        ]
    );
    // Nothing is emitted before SYN_REPORT.
    assert!(feed(&mut d, &[(EV_KEY, 31, 0)]).is_empty());
    assert_eq!(
        feed(&mut d, &[(EV_SYN, SYN_REPORT, 0)]),
        vec![InputEventKind::Key {
            code: 31,
            state: KeyState::Released
        }]
    );
}

#[test]
fn absolute_pointer_like_the_ultar_kernel_mouse() {
    let mut d = decoder(false);
    let got = feed(
        &mut d,
        &[
            (EV_ABS, ABS_X, 500),
            (EV_ABS, ABS_Y, 1000),
            (EV_SYN, SYN_REPORT, 0),
        ],
    );
    assert_eq!(
        got,
        vec![InputEventKind::PointerAbsolute { x: 32767, y: 65535 }]
    );
    // Only Y changes: X is kept.
    let got = feed(&mut d, &[(EV_ABS, ABS_Y, 0), (EV_SYN, SYN_REPORT, 0)]);
    assert_eq!(
        got,
        vec![InputEventKind::PointerAbsolute { x: 32767, y: 0 }]
    );
}

#[test]
fn single_touch_screen() {
    let mut d = decoder(false);
    let got = feed(
        &mut d,
        &[
            (EV_KEY, BTN_TOUCH, 1),
            (EV_ABS, ABS_X, 0),
            (EV_ABS, ABS_Y, 0),
            (EV_SYN, SYN_REPORT, 0),
        ],
    );
    assert_eq!(
        got,
        vec![InputEventKind::TouchDown {
            slot: 0,
            x: 0,
            y: 0
        }]
    );
    let got = feed(&mut d, &[(EV_ABS, ABS_X, 1000), (EV_SYN, SYN_REPORT, 0)]);
    assert_eq!(
        got,
        vec![InputEventKind::TouchMove {
            slot: 0,
            x: 65535,
            y: 0
        }]
    );
    let got = feed(&mut d, &[(EV_KEY, BTN_TOUCH, 0), (EV_SYN, SYN_REPORT, 0)]);
    assert_eq!(got, vec![InputEventKind::TouchUp { slot: 0 }]);
}

#[test]
fn multitouch_slots_and_dropped_events() {
    let mut d = decoder(true);
    let got = feed(
        &mut d,
        &[
            (EV_ABS, ABS_MT_SLOT, 0),
            (EV_ABS, ABS_MT_TRACKING_ID, 7),
            (EV_ABS, ABS_MT_POSITION_X, 1000),
            (EV_ABS, ABS_MT_POSITION_Y, 0),
            (EV_ABS, ABS_MT_SLOT, 1),
            (EV_ABS, ABS_MT_TRACKING_ID, 8),
            (EV_ABS, ABS_MT_POSITION_X, 0),
            (EV_SYN, SYN_REPORT, 0),
        ],
    );
    assert_eq!(
        got,
        vec![
            InputEventKind::TouchDown {
                slot: 0,
                x: 65535,
                y: 0
            },
            InputEventKind::TouchDown {
                slot: 1,
                x: 0,
                y: 0
            },
        ]
    );
    let got = feed(
        &mut d,
        &[
            (EV_ABS, ABS_MT_SLOT, 0),
            (EV_ABS, ABS_MT_TRACKING_ID, -1),
            (EV_SYN, SYN_REPORT, 0),
        ],
    );
    assert_eq!(got, vec![InputEventKind::TouchUp { slot: 0 }]);
    // Out-of-range slot numbers are clamped, never index out of bounds.
    feed(
        &mut d,
        &[(EV_ABS, ABS_MT_SLOT, 999), (EV_ABS, ABS_MT_SLOT, -4)],
    );
    let got = feed(&mut d, &[(EV_SYN, SYN_DROPPED, 0)]);
    assert_eq!(got, vec![InputEventKind::TouchCancel]);
}

#[test]
fn pending_events_are_bounded() {
    let mut d = decoder(false);
    let mut out = Vec::new();
    for _ in 0..1000 {
        d.feed(&record(EV_KEY, 30, 1), &mut out);
    }
    d.feed(&record(EV_SYN, SYN_REPORT, 0), &mut out);
    assert_eq!(out.len(), MAX_PENDING);
}

#[test]
fn files_that_are_not_devices_are_read_as_records() {
    // A regular file stands in for a device: classification falls back,
    // records are decoded, partial records wait for the rest.
    let path = std::env::temp_dir().join(format!("ultar-gfx-evdev-{}", std::process::id()));
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(&record(EV_KEY, 30, 1)).unwrap();
    file.write_all(&record(EV_SYN, SYN_REPORT, 0)).unwrap();
    file.write_all(&[1, 2, 3]).unwrap();
    let evdev = Evdev::open_paths(&[path.clone(), PathBuf::from("/nonexistent/event9")]).unwrap();
    let devices = evdev.devices().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].kind, InputDeviceKind::Keyboard);
    assert_eq!(evdev.poll_handles().len(), 1);
    let events = evdev.poll(0).unwrap();
    assert_eq!(events.len(), 1);
    assert!(evdev.poll(0).unwrap().is_empty());
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        Evdev::open_paths(&[PathBuf::from("/nonexistent/event0")]).err(),
        Some(GfxError::NoDevice)
    );
}
