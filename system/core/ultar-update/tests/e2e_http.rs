//! End to end over real HTTP against an in-process server that imitates (and abuses) the two
//! update endpoints of the Ultar server.

mod common;
#[path = "common/http.rs"]
mod http;

use common::*;
use http::{Behaviour, TestServer};
use ultar_update::bootctl::Slot;
use ultar_update::commit::BootOutcome;
use ultar_update::error::{NetworkError, UpdateError};
use ultar_update::net::HttpSource;
use ultar_update::stage::NoPacing;

fn server_with(release: &Release) -> TestServer {
    TestServer::start(Behaviour {
        file: release.bundle.clone(),
        latest: Some(TestServer::latest_json(release.bundle.len() as u64)),
        ..Behaviour::default()
    })
}

fn update(device: &Device, source: &mut HttpSource) -> Result<(), UpdateError> {
    let mut engine = device.engine();
    let offer = engine
        .check(source, "stable")?
        .ok_or(UpdateError::NoUpdate)?;
    engine.download(source, &offer, &mut NoPacing, &mut |_, _| {})?;
    engine.apply(&mut |_, _, _| {}).map(|_| ())
}

#[test]
fn full_update_over_http() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let server = server_with(&new);
    let mut source = HttpSource::new(&server.base, None).unwrap();
    update(&device, &mut source).unwrap();
    assert_eq!(device.reboot(&[&new]), (Slot::B, 1));
    let outcome = device.engine().resume_after_boot(&Health(true)).unwrap();
    assert_eq!(
        outcome,
        BootOutcome::Committed {
            version: Some(new.manifest.version)
        }
    );

    let seen = server.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(
        seen[0].target,
        "/v1/update/latest?channel=stable&arch=x86_64&edition=desktop"
    );
    assert_eq!(seen[1].target, "/v1/update/file/7");
    assert_eq!(
        seen[1].range.as_deref(),
        Some("bytes=0-131071"),
        "header probe only"
    );
    assert_eq!(
        seen[2].range,
        Some(format!("bytes={}-", new.header.raw.len()))
    );
    assert!(
        seen.iter().all(|s| s.authorization.is_none()),
        "no account needed"
    );
}

#[test]
fn broken_connection_is_resumed_with_a_range_request() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let server = server_with(&new);
    server.set(|b| b.drop_after = Some(120_000));
    let mut source = HttpSource::new(&server.base, None).unwrap();
    let first = update(&device, &mut source).unwrap_err();
    assert!(
        matches!(
            first,
            UpdateError::Truncated | UpdateError::Network(NetworkError::Transport)
        ),
        "{first}"
    );
    assert!(device.slot_b_untouched());
    server.set(|b| b.drop_after = None);
    update(&device, &mut source).unwrap();
    let ranges: Vec<String> = server
        .seen()
        .iter()
        .filter_map(|s| s.range.clone())
        .collect();
    let resumed = ranges.last().unwrap();
    let offset: usize = resumed
        .trim_start_matches("bytes=")
        .trim_end_matches('-')
        .parse()
        .unwrap();
    assert!(
        offset > new.header.raw.len(),
        "resumed from the middle: {resumed}"
    );
    assert!(offset <= new.header.raw.len() + 120_000);
    assert_eq!(device.images(Slot::B), new.payloads);
}

#[test]
fn server_without_range_support_still_works() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let server = server_with(&new);
    server.set(|b| b.ignore_range = true);
    let mut source = HttpSource::new(&server.base, None).unwrap();
    update(&device, &mut source).unwrap();
    assert_eq!(device.images(Slot::B), new.payloads);
}

#[test]
fn nothing_published_and_server_errors() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let server = server_with(&new);
    let mut source = HttpSource::new(&server.base, None).unwrap();
    server.set(|b| b.latest = None);
    assert!(device
        .engine()
        .check(&mut source, "stable")
        .unwrap()
        .is_none());
    for (status, check) in [
        (
            401u16,
            (|e| matches!(e, UpdateError::AuthRequired)) as fn(&UpdateError) -> bool,
        ),
        (403, |e| matches!(e, UpdateError::NotEntitled)),
        (404, |e| matches!(e, UpdateError::UnknownChannel)),
        (429, |e| matches!(e, UpdateError::RateLimited)),
        (500, |e| {
            matches!(e, UpdateError::Network(NetworkError::Status(500)))
        }),
    ] {
        server.set(|b| b.latest_status = Some(status));
        let error = device.engine().check(&mut source, "stable").unwrap_err();
        assert!(check(&error), "{status}: {error}");
    }
    drop(server);
    let error = device.engine().check(&mut source, "stable").unwrap_err();
    assert!(
        matches!(error, UpdateError::Network(NetworkError::Transport)),
        "{error}"
    );
    assert!(device.slot_b_untouched());
}

#[test]
fn hostile_answers_of_update_latest() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let server = server_with(&new);
    let mut source = HttpSource::new(&server.base, None).unwrap();
    for body in [
        "not json".to_string(),
        "{}".to_string(),
        r#"{"url":7}"#.to_string(),
        r#"{"url":"file:///etc/passwd"}"#.to_string(),
        r#"{"url":"relative/path"}"#.to_string(),
        r#"{"url":"/v1/update/file/7\r\nX-Injected: 1"}"#.to_string(),
        format!(r#"{{"url":"/{}"}}"#, "a".repeat(5000)),
        format!(
            r#"{{"url":"/v1/update/file/7","pad":"{}"}}"#,
            "x".repeat(70_000)
        ),
    ] {
        server.set(|b| b.latest = Some(body.clone()));
        let error = device.engine().check(&mut source, "stable").unwrap_err();
        assert!(
            matches!(error, UpdateError::Network(NetworkError::Protocol)),
            "{}: {error}",
            &body[..body.len().min(40)]
        );
    }
    // Wrong size, hash and version in the answer are simply ignored: only the manifest counts.
    server.set(|b| {
        b.latest =
            Some(r#"{"url":"/v1/update/file/7","size":1,"sha256":"ff","version":"0.0.1"}"#.into());
    });
    update(&device, &mut source).unwrap();
    assert_eq!(device.images(Slot::B), new.payloads);
}

#[test]
fn malicious_server_cannot_install_anything_unsigned_old_or_oversized() {
    let device = Device::new();
    let good = release((0, 7, 1), 2);
    let server = server_with(&good);
    let mut source = HttpSource::new(&server.base, None).unwrap();

    let foreign = release_with((0, 9, 0), 9, &STRANGER, |_| {});
    server.set(|b| b.file = foreign.bundle.clone());
    assert!(matches!(
        update(&device, &mut source),
        Err(UpdateError::UnknownKey)
    ));

    let mut tampered = good.bundle.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    server.set(|b| b.file = tampered.clone());
    assert!(matches!(
        update(&device, &mut source),
        Err(UpdateError::HashMismatch(_))
    ));

    let mut oversized = good.bundle.clone();
    oversized.extend_from_slice(&[0u8; 4096]);
    server.set(|b| b.file = oversized.clone());
    assert!(matches!(
        update(&device, &mut source),
        Err(UpdateError::SizeExceeded)
    ));

    server.set(|b| b.file = b"UUP1\xff\xff\xff\xff".to_vec());
    assert!(matches!(
        update(&device, &mut source),
        Err(UpdateError::Malformed(_))
    ));
    assert!(device.slot_b_untouched());

    // Install the good release, then let the server replay an older signed one.
    server.set(|b| b.file = good.bundle.clone());
    update(&device, &mut source).unwrap();
    device.reboot(&[&good]);
    device.engine().resume_after_boot(&Health(true)).unwrap();
    let old = release((0, 7, 0), 1);
    let disguised = release((0, 8, 0), 1);
    for replay in [&old, &disguised] {
        server.set(|b| b.file = replay.bundle.clone());
        assert!(matches!(
            update(&device, &mut source),
            Err(UpdateError::RollbackRefused {
                floor: 2,
                offered: 1
            })
        ));
    }
    assert_eq!(device.reboot(&[&good]), (Slot::B, 1));
}

#[test]
fn lying_content_range_is_refused() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let server = server_with(&new);
    let mut source = HttpSource::new(&server.base, None).unwrap();
    server.set(|b| b.fake_range_start = Some(5));
    assert!(matches!(
        update(&device, &mut source),
        Err(UpdateError::Network(NetworkError::Protocol))
    ));
    assert!(device.slot_b_untouched());
}

#[test]
fn gated_channel_needs_the_token_and_the_token_never_leaves_the_server() {
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let server = server_with(&new);
    server.set(|b| b.token = Some("s3cret-token".into()));
    let mut anonymous = HttpSource::new(&server.base, None).unwrap();
    assert!(matches!(
        update(&device, &mut anonymous),
        Err(UpdateError::AuthRequired)
    ));

    // The file lives on a mirror (another origin): the token must not be sent there.
    let mirror = server_with(&new);
    let location = format!("{}/mirror/7", mirror.base);
    server.set(|b| b.redirect = Some(location.clone()));
    let mut signed_in = HttpSource::new(&server.base, Some("s3cret-token".into())).unwrap();
    update(&device, &mut signed_in).unwrap();
    assert_eq!(device.images(Slot::B), new.payloads);
    assert!(server
        .seen()
        .iter()
        .skip(1)
        .all(|s| s.authorization.as_deref() == Some("Bearer s3cret-token")));
    let at_mirror = mirror.seen();
    assert!(at_mirror.len() >= 2);
    assert!(
        at_mirror.iter().all(|s| s.authorization.is_none()),
        "{at_mirror:?}"
    );
    assert!(at_mirror.iter().all(|s| s.target == "/mirror/7"));
}
