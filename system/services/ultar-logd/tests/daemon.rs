//! The real `ultar-logd`: ingestion with kernel credentials, reading
//! restricted by the policy, following, garbage on the socket, start errors.

mod common;

use common::world::{stop, World, LOGD};
use common::WAIT;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use ultar_bus::messages::log::{Query, QueryReply, SourceKind, Stats};
use ultar_bus::BusError;
use ultar_service::log::{Level, Record};

fn keys(reply: &QueryReply) -> Vec<&str> {
    reply
        .records
        .iter()
        .map(|record| record.key.as_str())
        .collect()
}

/// A running daemon into which two sources have written three records.
fn written(tag: &str) -> (World, std::process::Child) {
    let world = World::new(tag);
    let child = world.start(&[]);
    let one = world.logger("ingest.sock");
    let two = world.logger("two.sock");
    one.emit(Record::new(Level::Info, "one.started").count("n", 1));
    one.emit(Record::new(Level::Error, "one.failed").word("why", "timeout"));
    two.emit(Record::new(Level::Warn, "two.event"));
    assert!(one.flush(WAIT) && two.flush(WAIT));
    let admin = world.client("admin.support");
    world.wait_for_key(&admin, "two.event");
    world.wait_for_key(&admin, "one.failed");
    (world, child)
}

#[test]
fn sources_are_named_by_the_kernel_and_the_administrator_sees_all_of_them() {
    let (world, child) = written("admin");
    let admin = world.client("admin.support");
    let all = world.query(&admin, &Query::default());
    assert!(all.all_sources);
    let by = |name: &str| -> Vec<&str> {
        all.records
            .iter()
            .filter(|record| record.service == name)
            .map(|record| record.key.as_str())
            .collect()
    };
    // The writer on the credential socket is `app.one` because the kernel
    // says which user it is – nothing in its records names it.
    assert_eq!(by("app.one"), ["one.started", "one.failed"]);
    assert_eq!(by("app.two"), ["two.event"]);
    let kernel: Vec<_> = all
        .records
        .iter()
        .filter(|r| r.source == SourceKind::Kernel)
        .collect();
    assert_eq!(kernel.len(), 2);
    assert_eq!(kernel[0].text.as_deref(), Some("[kern] bereit nach 145 ms"));
    assert!(by("logd").contains(&"log.started"));
    // Statistics: the per-source table is for the administrator only.
    let stats = admin.call(&Stats {}, WAIT).unwrap();
    assert!(stats
        .sources
        .iter()
        .any(|s| s.service == "app.one" && s.stored == 2));
    let app = world.client("app.one");
    assert!(app.call(&Stats {}, WAIT).unwrap().sources.is_empty());
    assert_eq!(stop(child).0, Some(0));
}

#[test]
fn an_app_reads_its_own_records_and_nothing_else() {
    let (world, child) = written("own");
    let app = world.client("app.one");
    let own = world.query(&app, &Query::default());
    assert!(!own.all_sources);
    assert_eq!(keys(&own), ["one.started", "one.failed"]);
    assert!(own
        .records
        .iter()
        .all(|r| r.service == "app.one" && r.text.is_none()));
    // Asking for somebody else's records gives nothing – not even a hint.
    for other in ["app.two", "kernel", "logd"] {
        let query = Query {
            service: Some(other.to_owned()),
            ..Query::default()
        };
        assert!(world.query(&app, &query).records.is_empty(), "{other}");
    }
    let two = world.client("app.two");
    assert_eq!(keys(&world.query(&two, &Query::default())), ["two.event"]);
    // Filters: level and position.
    let errors = Query {
        min_level: 4,
        ..Query::default()
    };
    assert_eq!(keys(&world.query(&app, &errors)), ["one.failed"]);
    let after = Query {
        after_seq: own.records[0].seq,
        ..Query::default()
    };
    assert_eq!(keys(&world.query(&app, &after)), ["one.failed"]);
    // An identity without any rule is refused by the broker already.
    let mute = world.client("other.mute");
    assert!(matches!(
        mute.call(&Query::default(), WAIT),
        Err(BusError::Remote(ultar_bus::ErrorCode::PermissionDenied))
    ));
    assert_eq!(stop(child).0, Some(0));
}

#[test]
fn a_following_reader_gets_new_records_without_polling() {
    let world = World::new("follow");
    let child = world.start(&[]);
    let admin = world.client("admin.support");
    let start = world.wait_for_key(&admin, "log.bus_joined").next_seq;
    let follower = {
        let client = world.client("app.one");
        std::thread::spawn(move || {
            let query = Query {
                after_seq: start,
                wait_ms: 30_000,
                ..Query::default()
            };
            client.call(&query, WAIT + WAIT).unwrap()
        })
    };
    // Written by another source first: must not wake the app's reader
    // with a foreign record.
    let two = world.logger("two.sock");
    two.log(Level::Info, "two.noise");
    assert!(two.flush(WAIT));
    world.wait_for_key(&admin, "two.noise");
    let one = world.logger("ingest.sock");
    one.log(Level::Info, "one.awaited");
    let reply = follower.join().unwrap();
    assert_eq!(keys(&reply), ["one.awaited"]);
    // A short wait without news ends empty and tells where to continue.
    let quiet = Query {
        after_seq: reply.next_seq,
        wait_ms: 1,
        service: Some("app.one".into()),
        ..Query::default()
    };
    let empty = world.query(&world.client("app.one"), &quiet);
    assert!(empty.records.is_empty() && empty.next_seq >= reply.next_seq);
    assert_eq!(stop(child).0, Some(0));
}

#[test]
fn garbage_on_the_socket_is_counted_and_harms_nobody() {
    let world = World::new("garbage");
    let child = world.start(&[]);
    let mut raw = std::os::unix::net::UnixStream::connect(world.path("ingest.sock")).unwrap();
    // A well-framed body that is no record, then a frame length of zero.
    raw.write_all(&[3, 0, 0xff, 0xff, 0xff]).unwrap();
    raw.write_all(&[0, 0]).unwrap();
    // The service hangs up on a stream that lost its framing.
    let mut rest = Vec::new();
    std::io::Read::read_to_end(&mut raw, &mut rest).unwrap();
    let one = world.logger("ingest.sock");
    one.log(Level::Info, "one.fine");
    assert!(one.flush(WAIT));
    let admin = world.client("admin.support");
    world.wait_for_key(&admin, "one.fine");
    let stats = admin.call(&Stats {}, WAIT).unwrap();
    let source = stats
        .sources
        .iter()
        .find(|s| s.service == "app.one")
        .unwrap();
    assert_eq!((source.stored, source.malformed), (1, 2));
    assert_eq!(stop(child).0, Some(0));
}

#[test]
fn start_errors_are_reported_and_nothing_is_left_behind() {
    let world = World::new("errors");
    let run = |command: &mut Command| -> (Option<i32>, String) {
        let output = command.output().unwrap();
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    // The chain key must be private.
    std::fs::set_permissions(
        world.path("chain.key"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let (code, stderr) = run(&mut world.logd(&[]));
    assert_eq!(code, Some(1));
    assert_eq!(stderr, "Secret file rejected: secret.too_permissive\n");
    std::fs::set_permissions(
        world.path("chain.key"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    // A uid map others can write is refused.
    std::fs::set_permissions(
        world.path("uids.toml"),
        std::fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    let (code, stderr) = run(&mut world.logd(&[]));
    assert_eq!(code, Some(1));
    assert!(
        stderr.starts_with("User-to-identity map rejected:"),
        "{stderr}"
    );
    std::fs::set_permissions(
        world.path("uids.toml"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    // Limits that do not fit together.
    let (code, stderr) = run(&mut world.logd(&["--total-bytes", "20000"]));
    assert_eq!(code, Some(2));
    assert!(
        stderr.starts_with("The given limits do not fit together."),
        "{stderr}"
    );
    // A file where the socket should be is not removed.
    std::fs::write(world.path("ingest.sock"), b"keep me").unwrap();
    let (code, stderr) = run(&mut world.logd(&[]));
    assert_eq!(code, Some(1));
    assert!(stderr.contains("not a socket"), "{stderr}");
    assert_eq!(
        std::fs::read(world.path("ingest.sock")).unwrap(),
        b"keep me"
    );
}

#[test]
fn root_must_be_declared() {
    if ultar_service::effective_uid() == 0 {
        let output = Command::new(LOGD)
            .args(["--lang", "en", "--dir", "/tmp/x", "--listen", "/tmp/y=svc"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(71));
    }
}
