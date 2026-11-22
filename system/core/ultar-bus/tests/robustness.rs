//! The broker survives hostile, broken and slow peers.

mod common;

use common::{client, eventually, open_broker, start, start_at, RawPeer, QUIET, WAIT};
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};
use ultar_bus::wire::{write_preamble, Envelope, Kind, MAGIC, PROTOCOL_VERSION};
use ultar_bus::{
    Broker, BrokerConfig, BusError, Client, ClientOptions, ErrorCode, FixedIdentity, Policy,
};

fn small_queue(write_timeout: Duration) -> BrokerConfig {
    BrokerConfig {
        queue_capacity: 8,
        write_timeout,
        ..BrokerConfig::default()
    }
}

fn flood(publisher: &Client, count: usize) {
    for _ in 0..count {
        publisher
            .publish_raw("flood.data", 1, false, vec![0xAB; 32 * 1024], false)
            .unwrap();
    }
    // Confirmed: the broker has processed everything sent before.
    publisher
        .publish_raw("flood.done", 1, false, Vec::new(), true)
        .unwrap();
}

fn assert_bus_works(path: &std::path::Path) {
    let subscriber = client(path, "check-sub");
    let stream = subscriber.subscribe_pattern("check.topic").unwrap();
    let publisher = client(path, "check-pub");
    publisher
        .publish_raw("check.topic", 1, false, vec![1, 2, 3], true)
        .unwrap();
    assert_eq!(stream.recv_timeout(WAIT).unwrap().payload, vec![1, 2, 3]);
}

#[test]
fn subscriber_that_never_reads_is_dropped_without_blocking_anyone() {
    let (broker, path) = start(
        "never-reads",
        Policy::allow_all(),
        small_queue(Duration::from_millis(300)),
    );
    let mut stuck = RawPeer::connect(&path);
    stuck.subscribe(1, "flood.*");
    let healthy = Client::connect(
        &path,
        ClientOptions {
            buffer: 4096,
            ..ClientOptions::named("healthy")
        },
    )
    .unwrap();
    let stream = healthy.subscribe_pattern("flood.data").unwrap();
    let publisher = client(&path, "publisher");

    // The healthy subscriber takes every event right away, so only the
    // stuck peer falls behind.
    let started = Instant::now();
    for _ in 0..200 {
        publisher
            .publish_raw("flood.data", 1, false, vec![0xAB; 32 * 1024], false)
            .unwrap();
        assert_eq!(stream.recv_timeout(WAIT).unwrap().payload.len(), 32 * 1024);
    }
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "publisher was blocked"
    );
    assert_eq!(broker.stats().dropped_slow, 1);
    eventually("stuck peer removed", || broker.stats().connections == 2);
    assert_bus_works(&path);
}

#[test]
fn slow_subscriber_gets_the_typed_error_as_last_frame() {
    let (broker, path) = start(
        "slow-typed",
        Policy::allow_all(),
        small_queue(Duration::from_secs(20)),
    );
    let mut slow = RawPeer::connect(&path);
    slow.subscribe(1, "flood.*");
    let publisher = client(&path, "publisher");
    flood(&publisher, 200);
    assert_eq!(broker.stats().dropped_slow, 1);
    // Now the peer wakes up and reads what is left for it.
    let mut received = 0;
    let last = loop {
        match slow.recv() {
            Ok(envelope) if envelope.kind == Kind::Event => received += 1,
            other => break other,
        }
    };
    let last = last.unwrap();
    assert_eq!(
        (last.kind, last.id, last.error_code()),
        (Kind::Error, 0, ErrorCode::SlowConsumer)
    );
    assert!(received < 200, "the peer cannot have received everything");
    assert!(matches!(slow.recv(), Err(BusError::Closed)));
}

#[test]
fn oversized_frame_ends_only_that_connection() {
    let config = BrokerConfig {
        max_frame: 1024,
        ..BrokerConfig::default()
    };
    let (broker, path) = start("oversized", Policy::allow_all(), config);
    let mut peer = RawPeer::connect(&path);
    peer.stream.write_all(&(1025u32).to_le_bytes()).unwrap();
    let answer = peer.recv().unwrap();
    assert_eq!(
        (answer.kind, answer.error_code()),
        (Kind::Error, ErrorCode::FrameTooLarge)
    );
    assert!(matches!(peer.recv(), Err(BusError::Closed)));

    let mut huge = RawPeer::connect(&path);
    huge.stream.write_all(&u32::MAX.to_le_bytes()).unwrap();
    assert_eq!(huge.recv().unwrap().error_code(), ErrorCode::FrameTooLarge);
    eventually("both removed", || broker.stats().connections == 0);
    assert_bus_works(&path);
}

#[test]
fn client_refuses_to_send_an_oversized_frame_and_stays_connected() {
    let config = BrokerConfig {
        max_frame: 1024,
        ..BrokerConfig::default()
    };
    let (_broker, path) = start("oversized-local", Policy::allow_all(), config);
    let app = client(&path, "app");
    let result = app.publish_raw("big.topic", 1, false, vec![0; 4096], true);
    assert!(matches!(
        result,
        Err(BusError::FrameTooLarge { max: 1024, .. })
    ));
    assert!(app.is_connected());
    app.publish_raw("small.topic", 1, false, vec![0; 16], true)
        .unwrap();
}

#[test]
fn malformed_frames_end_only_that_connection() {
    let (broker, path) = open_broker("malformed");
    let bodies: [&[u8]; 4] = [&[0xFF; 16], &[0x63], &[4, 0, 0], &[0u8; 0]];
    for body in bodies {
        let mut peer = RawPeer::connect(&path);
        peer.stream
            .write_all(&(body.len() as u32).to_le_bytes())
            .unwrap();
        peer.stream.write_all(body).unwrap();
        let answer = peer.recv().unwrap();
        assert_eq!(
            (answer.kind, answer.error_code()),
            (Kind::Error, ErrorCode::Malformed)
        );
        assert!(matches!(peer.recv(), Err(BusError::Closed)));
    }
    let mut violator = RawPeer::connect(&path);
    violator.send(&Envelope::new(Kind::Hello, 0));
    assert_eq!(
        violator.recv().unwrap().error_code(),
        ErrorCode::ProtocolViolation
    );
    eventually("all removed", || broker.stats().connections == 0);
    assert_bus_works(&path);
}

#[test]
fn abrupt_disconnects_leave_nothing_behind() {
    let (broker, path) = open_broker("abrupt");
    for round in 0..30u64 {
        let mut peer = RawPeer::connect(&path);
        peer.subscribe(1, "*");
        peer.send(&Envelope::new(Kind::Register, 2).with_target("victim"));
        if round % 3 == 0 {
            // Half a frame, then gone.
            peer.stream.write_all(&[200, 0, 0, 0, 1, 2, 3]).unwrap();
        }
        if round % 3 == 1 {
            // Header only.
            peer.stream.write_all(&[9, 0]).unwrap();
        }
        drop(peer);
        eventually("peer cleaned up", || broker.stats().connections == 0);
    }
    let stats = broker.stats();
    assert_eq!(
        (stats.services, stats.subscriptions, stats.pending_requests),
        (0, 0, 0)
    );
    assert_bus_works(&path);
}

#[test]
fn silent_and_foreign_peers_are_turned_away() {
    let config = BrokerConfig {
        handshake_timeout: Duration::from_millis(100),
        ..BrokerConfig::default()
    };
    let (broker, path) = start("handshake", Policy::allow_all(), config);

    // Says nothing: dropped after the handshake timeout.
    let mut silent = RawPeer::open(&path);
    let mut byte = [0u8; 1];
    let started = Instant::now();
    assert_eq!(silent.stream.read(&mut byte).unwrap(), 0);
    assert!(started.elapsed() < Duration::from_secs(5));

    // Speaks another protocol: dropped without an answer.
    let mut foreign = RawPeer::open(&path);
    foreign.stream.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
    assert_eq!(foreign.stream.read(&mut byte).unwrap_or(0), 0);

    // Correct preamble but no `Hello`.
    let mut rude = RawPeer::open(&path);
    write_preamble(&mut rude.stream, PROTOCOL_VERSION).unwrap();
    let mut preamble = [0u8; 8];
    rude.stream.read_exact(&mut preamble).unwrap();
    rude.send(&Envelope::new(Kind::Subscribe, 1).with_target("*"));
    assert_eq!(
        rude.recv().unwrap().error_code(),
        ErrorCode::ProtocolViolation
    );

    assert_eq!(broker.stats().connections, 0);
    assert_bus_works(&path);
}

#[test]
fn broker_announces_its_version_to_a_peer_with_another_one() {
    let (_broker, path) = open_broker("version-broker");
    let mut peer = RawPeer::open(&path);
    write_preamble(&mut peer.stream, PROTOCOL_VERSION + 41).unwrap();
    let mut preamble = [0u8; 8];
    peer.stream.read_exact(&mut preamble).unwrap();
    assert_eq!(preamble[..4], MAGIC);
    assert_eq!(
        u16::from_le_bytes([preamble[4], preamble[5]]),
        PROTOCOL_VERSION
    );
    let mut byte = [0u8; 1];
    assert_eq!(
        peer.stream.read(&mut byte).unwrap_or(0),
        0,
        "connection must be closed"
    );
    assert_bus_works(&path);
}

/// A listener that answers the preamble with `version` and then runs `script`.
fn fake_broker(
    tag: &str,
    version: u16,
    script: impl FnOnce(&mut RawPeer) + Send + 'static,
) -> std::path::PathBuf {
    let path = common::socket_path(tag);
    let listener = UnixListener::bind(&path).unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut peer = RawPeer { stream };
        let mut preamble = [0u8; 8];
        peer.stream.read_exact(&mut preamble).unwrap();
        write_preamble(&mut peer.stream, version).unwrap();
        script(&mut peer);
    });
    path
}

#[test]
fn client_reports_protocol_version_mismatch() {
    let path = fake_broker("version-client", PROTOCOL_VERSION + 1, |_| {});
    let result = Client::connect(&path, ClientOptions::named("old-client"));
    assert!(matches!(
        result,
        Err(BusError::ProtocolMismatch { ours, theirs })
            if ours == PROTOCOL_VERSION && theirs == PROTOCOL_VERSION + 1
    ));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn client_rejects_a_peer_that_is_not_a_bus() {
    let path = common::socket_path("not-a-bus");
    let listener = UnixListener::bind(&path).unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = stream.write_all(b"220 smtp ready\r\n");
        std::thread::sleep(QUIET);
    });
    let result = Client::connect(&path, ClientOptions::named("client"));
    assert!(matches!(result, Err(BusError::BadMagic)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn client_subscription_learns_that_the_broker_dropped_it_as_slow() {
    let path = fake_broker("lag-notice", PROTOCOL_VERSION, |peer| {
        let hello = peer.recv().unwrap();
        assert_eq!(hello.kind, Kind::Hello);
        let welcome = ultar_bus::wire::Welcome {
            identity: "test.peer".into(),
            max_frame: 65536,
        };
        peer.send(
            &Envelope::new(Kind::Welcome, 0).with_payload(1, ultar_bus::encode(&welcome).unwrap()),
        );
        let subscribe = peer.recv().unwrap();
        assert_eq!(subscribe.kind, Kind::Subscribe);
        peer.send(&Envelope::new(Kind::Response, subscribe.id));
        peer.send(&Envelope::error(0, ErrorCode::SlowConsumer));
    });
    let options = ClientOptions {
        reconnect: false,
        ..ClientOptions::named("client")
    };
    let slow = Client::connect(&path, options).unwrap();
    let stream = slow.subscribe_pattern("some.topic").unwrap();
    assert!(matches!(
        stream.recv_timeout(WAIT),
        Err(BusError::Lagged(1))
    ));
    assert!(matches!(stream.recv_timeout(WAIT), Err(BusError::Closed)));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn connection_limit_is_enforced() {
    let config = BrokerConfig {
        max_connections: 2,
        ..BrokerConfig::default()
    };
    let (broker, path) = start("limit", Policy::allow_all(), config);
    let first = client(&path, "first");
    let _second = client(&path, "second");
    let third = Client::connect(&path, ClientOptions::named("third"));
    assert!(matches!(
        third,
        Err(BusError::Remote(ErrorCode::LimitExceeded))
    ));
    drop(first);
    eventually("slot freed", || broker.stats().connections == 1);
    assert!(Client::connect(&path, ClientOptions::named("third")).is_ok());
}

#[test]
fn shutdown_is_clean_and_prompt() {
    let (broker, path) = open_broker("shutdown");
    let options = ClientOptions {
        reconnect: false,
        ..ClientOptions::named("client")
    };
    let app = Client::connect(&path, options).unwrap();
    let stream = app.subscribe_pattern("*").unwrap();
    let endpoint = app.register("svc").unwrap();
    let mut raw = RawPeer::connect(&path);
    let _idle = RawPeer::open(&path);

    let started = Instant::now();
    broker.shutdown();
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "shutdown took {:?}",
        started.elapsed()
    );
    assert!(!path.exists(), "socket file must be removed");

    let last = raw.recv().unwrap();
    assert_eq!(
        (last.kind, last.error_code()),
        (Kind::Error, ErrorCode::ShuttingDown)
    );
    assert!(matches!(stream.recv_timeout(WAIT), Err(BusError::Closed)));
    assert!(matches!(endpoint.recv_timeout(WAIT), Err(BusError::Closed)));
    assert!(!app.wait_connected(QUIET));
    assert!(matches!(
        app.publish_raw("a.b", 1, false, Vec::new(), true),
        Err(BusError::Closed)
    ));
}

#[test]
fn stale_socket_file_is_replaced_but_a_live_broker_is_respected() {
    let path = common::socket_path("stale");
    // A socket file nobody listens on (listener dropped without cleanup).
    drop(UnixListener::bind(&path).unwrap());
    assert!(path.exists());
    let broker = start_at(&path, Policy::allow_all(), BrokerConfig::default());
    assert_bus_works(&path);

    let auth = std::sync::Arc::new(FixedIdentity::new(common::id("second")));
    let second = Broker::builder(Policy::allow_all())
        .listen(&path, auth)
        .start();
    assert!(matches!(second, Err(BusError::AddressInUse)));
    assert_bus_works(&path);
    assert_eq!(broker.paths(), std::slice::from_ref(&path));
    drop(broker);
    assert!(!path.exists());
}

#[test]
fn unbindable_path_is_an_io_error() {
    let path = std::env::temp_dir().join("ubus-no-such-dir").join("x.sock");
    let auth = std::sync::Arc::new(FixedIdentity::new(common::id("x")));
    let result = Broker::builder(Policy::allow_all())
        .listen(&path, auth)
        .start();
    assert!(matches!(result, Err(BusError::Io(_))));
}
