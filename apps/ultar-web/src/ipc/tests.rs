use super::*;
use crate::engine::*;

fn events() -> Vec<HostEvent> {
    let mut all = navigation_events();
    all.extend(other_events());
    all
}

fn navigation_events() -> Vec<HostEvent> {
    let t = TabId(3);
    vec![
        HostEvent::Ready(PROTOCOL_VERSION),
        HostEvent::Buffer(t, 800, 600, 3200),
        HostEvent::Engine(EngineEvent::Navigated {
            tab: t,
            url: "https://a.example/ع".into(),
            can_back: true,
            can_forward: false,
        }),
        HostEvent::Engine(EngineEvent::Title {
            tab: t,
            title: "عنوان".into(),
        }),
        HostEvent::Engine(EngineEvent::Progress {
            tab: t,
            loading: true,
            progress: 0.5,
        }),
        HostEvent::Engine(EngineEvent::Security {
            tab: t,
            secure: true,
        }),
    ]
}

fn other_events() -> Vec<HostEvent> {
    let t = TabId(3);
    vec![
        HostEvent::Engine(EngineEvent::Frame {
            tab: t,
            damage: FrameDamage {
                x0: 1,
                y0: 2,
                x1: 3,
                y1: 4,
            },
        }),
        HostEvent::Engine(EngineEvent::Permission {
            tab: t,
            request: RequestId(9),
            host: "a.example".into(),
            kind: PermissionKind::Location,
        }),
        HostEvent::Engine(EngineEvent::Blocked { tab: t, count: 7 }),
        HostEvent::Engine(EngineEvent::DownloadStarted {
            download: DownloadId(1),
            url: "u".into(),
            name: "n".into(),
        }),
        HostEvent::Engine(EngineEvent::DownloadProgress {
            download: DownloadId(1),
            received: 1 << 40,
            total: 5,
        }),
        HostEvent::Engine(EngineEvent::DownloadFinished {
            download: DownloadId(1),
            ok: false,
        }),
        HostEvent::Engine(EngineEvent::NewTab {
            opener: t,
            url: "https://b/".into(),
        }),
        HostEvent::Engine(EngineEvent::Crashed { tab: t }),
    ]
}

#[test]
fn every_event_round_trips_through_the_decoder() {
    let mut d = Decoder::default();
    for e in events() {
        let bytes = e.encode();
        // Delivered in two pieces.
        d.push(&bytes[..3]);
        assert_eq!(d.next_message().unwrap(), None);
        d.push(&bytes[3..]);
        let (kind, payload) = d.next_message().unwrap().unwrap();
        assert_eq!(HostEvent::decode(kind, &payload).unwrap(), e);
    }
    assert_eq!(d.pending(), 0);
}

#[test]
fn commands_have_the_documented_layout() {
    let t = TabId(0x0102_0304);
    assert_eq!(Command::Back(t).encode(), vec![5, 0, 0, 0, 4, 4, 3, 2, 1]);
    assert_eq!(Command::Quit.encode(), vec![1, 0, 0, 0, 16]);
    let load = Command::Load(TabId(1), "ab".into()).encode();
    assert_eq!(load, vec![9, 0, 0, 0, 3, 1, 0, 0, 0, 2, 0, b'a', b'b']);
    let key = Command::Key(
        TabId(1),
        KeyInput {
            code: 30,
            character: 97,
            modifiers: 2,
            pressed: true,
        },
    );
    assert_eq!(key.encode().len(), 4 + 1 + 4 + 2 + 4 + 1 + 1);
    let p = Command::Pointer(
        TabId(1),
        PointerInput::Scroll {
            x: 1.0,
            y: 2.0,
            dx: 0.0,
            dy: 3.0,
        },
    );
    assert_eq!(p.encode().len(), 4 + 1 + 4 + 2 + 16);
    let long = Command::Load(t, "ع".repeat(MAX_STRING)).encode();
    assert!(long.len() <= 4 + 1 + 4 + 2 + MAX_STRING);
}

#[test]
fn every_command_has_a_correct_length_field() {
    let t = TabId(0x0102_0304);
    for c in [
        Command::CreateTab(t, true),
        Command::CloseTab(t),
        Command::Forward(t),
        Command::Reload(t),
        Command::Stop(t),
        Command::Resize(t, 1, 2, 1.5),
        Command::Visible(t, false),
        Command::Protection(t, true),
        Command::Permission(RequestId(1), true),
        Command::CancelDownload(DownloadId(2)),
        Command::FrameDone(t),
        Command::Pointer(
            t,
            PointerInput::Down {
                x: 0.0,
                y: 0.0,
                button: 1,
            },
        ),
        Command::Pointer(
            t,
            PointerInput::Up {
                x: 0.0,
                y: 0.0,
                button: 1,
            },
        ),
        Command::Pointer(t, PointerInput::Move { x: 0.0, y: 0.0 }),
    ] {
        let b = c.encode();
        assert_eq!(
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize,
            b.len() - 4
        );
    }
}

#[test]
fn every_command_round_trips_through_the_host_side() {
    let t = TabId(7);
    let key = KeyInput {
        code: 30,
        character: u32::from('ش'),
        modifiers: 3,
        pressed: true,
    };
    let scroll = PointerInput::Scroll {
        x: 1.0,
        y: 2.0,
        dx: 0.0,
        dy: -3.5,
    };
    let all = [
        Command::CreateTab(t, true),
        Command::CloseTab(t),
        Command::Load(t, "https://ar.wikipedia.org/wiki/سوريا".into()),
        Command::Back(t),
        Command::Forward(t),
        Command::Reload(t),
        Command::Stop(t),
        Command::Resize(t, 1400, 900, 2.0),
        Command::Resize(t, 0, 0, 1.0),
        Command::Visible(t, true),
        Command::Pointer(t, scroll),
        Command::Key(t, key),
        Command::Protection(t, false),
        Command::Permission(RequestId(4), false),
        Command::CancelDownload(DownloadId(5)),
        Command::FrameDone(t),
        Command::Quit,
    ];
    for c in all {
        let mut d = Decoder::default();
        d.push(&c.encode());
        let (kind, payload) = d.next_message().unwrap().unwrap();
        assert_eq!(Command::decode(kind, &payload).unwrap(), c);
    }
    let huge = Command::Resize(t, MAX_SIDE + 1, 1, 1.0).encode();
    assert_eq!(
        Command::decode(huge[4], &huge[5..]),
        Err(DecodeError::BadValue)
    );
    let mut trailing = Command::Back(t).encode();
    trailing.push(0);
    assert_eq!(
        Command::decode(trailing[4], &trailing[5..]),
        Err(DecodeError::TrailingBytes)
    );
    assert_eq!(Command::decode(99, &[]), Err(DecodeError::UnknownKind(99)));
}

#[test]
fn invalid_messages_are_refused() {
    let mut d = Decoder::default();
    d.push(&[0, 0, 0, 0, 1]);
    assert_eq!(d.next_message(), Err(DecodeError::BadLength));
    let mut d = Decoder::default();
    d.push(&((MAX_MESSAGE + 1) as u32).to_le_bytes());
    assert_eq!(d.next_message(), Err(DecodeError::BadLength));
    assert_eq!(
        HostEvent::decode(200, &[]),
        Err(DecodeError::UnknownKind(200))
    );
    assert_eq!(HostEvent::decode(101, &[1]), Err(DecodeError::Truncated));
    assert_eq!(
        HostEvent::decode(101, &[1, 0, 0]),
        Err(DecodeError::TrailingBytes)
    );
    assert_eq!(
        HostEvent::decode(105, &[1, 0, 0, 0, 2]),
        Err(DecodeError::BadValue)
    );
    let buffer = |w: u32, h: u32, s: u32| {
        let mut p = 1u32.to_le_bytes().to_vec();
        for v in [w, h, s] {
            p.extend_from_slice(&v.to_le_bytes());
        }
        HostEvent::decode(106, &p)
    };
    assert!(buffer(10, 10, 40).is_ok());
    assert_eq!(buffer(0, 10, 40), Err(DecodeError::BadValue));
    assert_eq!(buffer(10, 10, 39), Err(DecodeError::BadValue));
    assert_eq!(
        buffer(MAX_SIDE + 1, 1, u32::MAX),
        Err(DecodeError::BadValue)
    );
    let mut bad_utf8 = 1u32.to_le_bytes().to_vec();
    bad_utf8.extend_from_slice(&[1, 0, 0xff]);
    assert_eq!(HostEvent::decode(103, &bad_utf8), Err(DecodeError::BadUtf8));
    let mut nan = 1u32.to_le_bytes().to_vec();
    nan.push(1);
    nan.extend_from_slice(&f32::NAN.to_bits().to_le_bytes());
    assert_eq!(HostEvent::decode(104, &nan), Err(DecodeError::BadValue));
    let mut frame = 1u32.to_le_bytes().to_vec();
    for v in [5u32, 0, 5, 1] {
        frame.extend_from_slice(&v.to_le_bytes());
    }
    assert_eq!(HostEvent::decode(107, &frame), Err(DecodeError::BadValue));
    let mut perm = vec![1, 0, 0, 0, 1, 0, 0, 0, 9];
    perm.extend_from_slice(&[0, 0]);
    assert_eq!(HostEvent::decode(108, &perm), Err(DecodeError::BadValue));
    for e in [
        DecodeError::Truncated,
        DecodeError::UnknownKind(1),
        DecodeError::TooLong,
    ] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn fuzzed_bytes_never_panic() {
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let valid: Vec<Vec<u8>> = events().iter().map(HostEvent::encode).collect();
    for round in 0..30_000 {
        let mut bytes = valid[round % valid.len()].clone();
        for _ in 0..(next() % 4) {
            let i = (next() % bytes.len() as u64) as usize;
            bytes[i] = next() as u8;
        }
        bytes.truncate(4 + (next() % bytes.len() as u64) as usize);
        let mut d = Decoder::default();
        d.push(&bytes);
        if let Ok(Some((kind, payload))) = d.next_message() {
            let _ = HostEvent::decode(kind, &payload);
        }
    }
}
