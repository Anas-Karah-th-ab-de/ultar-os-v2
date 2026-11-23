//! The listener gate (review round 3, R3-B0/R3-B1): connections in the
//! handshake cost no thread, a full gate makes new peers wait instead of
//! failing, and the cost of a configuration is known before the start.

mod common;

use common::{client, start, WAIT};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use ultar_bus::{Broker, BrokerConfig, FixedIdentity, Needs, Policy, SocketAccess};

/// Threads of this process whose name starts with `prefix`.
fn threads_named(prefix: &str) -> usize {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(|task| std::fs::read_to_string(task.ok()?.path().join("comm")).ok())
        .filter(|name| name.starts_with(prefix))
        .count()
}

#[test]
fn silent_connections_cost_no_thread_and_do_not_lock_out_a_client() {
    let config = BrokerConfig {
        handshake_timeout: Duration::from_millis(200),
        ..BrokerConfig::default()
    };
    let (_broker, path) = start("gate-silent", Policy::allow_all(), config);
    let before = threads_named("ultar-bus-conn");
    let silent: Vec<UnixStream> = (0..100)
        .filter_map(|_| UnixStream::connect(&path).ok())
        .collect();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(threads_named("ultar-bus-conn"), before);
    // The gate has 16 slots; the client waits in the listen queue until
    // the silent peers time out, then gets in.
    let started = Instant::now();
    let _client = client(&path, "late");
    assert!(started.elapsed() < WAIT);
    drop(silent);
}

#[test]
fn the_cost_of_a_configuration_is_known_before_the_start() {
    let config = BrokerConfig {
        max_connections: 10,
        max_handshakes: 4,
        ..BrokerConfig::default()
    };
    let auth = || std::sync::Arc::new(FixedIdentity::new(common::id("x")));
    let own = SocketAccess {
        connections: Some(3),
        ..SocketAccess::mode(0o600)
    };
    let builder = Broker::builder(Policy::allow_all())
        .config(config)
        .listen(&common::socket_path("needs-a"), auth())
        .listen_with_access(&common::socket_path("needs-b"), auth(), own);
    // (10 + 4 + 1) + (3 + 4 + 1) descriptors, (2 * 10 + 1) + (2 * 3 + 1) threads.
    let expected = Needs {
        descriptors: 23,
        threads: 28,
    };
    assert_eq!(builder.needs(), expected);
}

#[test]
fn a_listener_limit_of_its_own_is_enforced() {
    let path = common::socket_path("own-limit");
    let access = SocketAccess {
        connections: Some(1),
        ..SocketAccess::default()
    };
    let _broker = Broker::builder(Policy::allow_all())
        .listen_with_access(
            &path,
            std::sync::Arc::new(FixedIdentity::new(common::id("x"))),
            access,
        )
        .start()
        .unwrap();
    let _first = client(&path, "first");
    let second = ultar_bus::Client::connect(&path, ultar_bus::ClientOptions::named("second"));
    assert!(matches!(
        second,
        Err(ultar_bus::BusError::Remote(
            ultar_bus::ErrorCode::LimitExceeded
        ))
    ));
}
