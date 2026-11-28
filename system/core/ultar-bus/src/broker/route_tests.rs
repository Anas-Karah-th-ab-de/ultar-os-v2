//! Unit tests of the routing step, without sockets or threads.

use super::route::{handle, Rules};
use super::state::testing::{add_conn, Probe};
use super::state::State;
use super::BrokerConfig;
use crate::error::ErrorCode;
use crate::policy::{Policy, Rule};
use crate::wire::{Envelope, Kind, FLAG_ACK, FLAG_RETAIN};

struct Bench {
    state: State,
    policy: Policy,
    config: BrokerConfig,
}

impl Bench {
    fn new(policy: Policy) -> Self {
        Self {
            state: State::new(),
            policy,
            config: BrokerConfig::default(),
        }
    }

    fn open() -> Self {
        Self::new(Policy::allow_all())
    }

    fn feed(&mut self, conn: u64, envelope: Envelope) -> Result<(), ErrorCode> {
        let rules = Rules {
            policy: &self.policy,
            config: &self.config,
        };
        handle(&mut self.state, &rules, conn, envelope)
    }

    fn join(&mut self, conn: u64, identity: &str) -> Probe {
        add_conn(&mut self.state, conn, identity, 16)
    }
}

fn envelope(kind: Kind, id: u64, target: &str) -> Envelope {
    Envelope::new(kind, id).with_target(target)
}

fn event(id: u64, topic: &str, flags: u8, payload: &[u8]) -> Envelope {
    let mut event = envelope(Kind::Event, id, topic).with_payload(1, payload.to_vec());
    event.flags = flags;
    event
}

fn call(id: u64, service: &str, method: &str) -> Envelope {
    let mut request = envelope(Kind::Request, id, service);
    request.method = method.into();
    request
}

fn expect_error(probe: &Probe, id: u64, code: ErrorCode) {
    let reply = probe.next().unwrap();
    assert_eq!(
        (reply.kind, reply.id, reply.error_code()),
        (Kind::Error, id, code)
    );
}

fn expect_ack(probe: &Probe, id: u64) {
    let reply = probe.next().unwrap();
    assert_eq!((reply.kind, reply.id), (Kind::Response, id));
}

#[test]
fn register_is_idempotent_and_exclusive() {
    let mut bench = Bench::open();
    let first = bench.join(1, "a");
    let second = bench.join(2, "b");
    bench
        .feed(1, envelope(Kind::Register, 1, "settings"))
        .unwrap();
    bench
        .feed(1, envelope(Kind::Register, 2, "settings"))
        .unwrap();
    expect_ack(&first, 1);
    expect_ack(&first, 2);
    bench
        .feed(2, envelope(Kind::Register, 3, "settings"))
        .unwrap();
    expect_error(&second, 3, ErrorCode::NameTaken);
    bench
        .feed(2, envelope(Kind::Register, 4, "Bad Name"))
        .unwrap();
    expect_error(&second, 4, ErrorCode::InvalidName);
    bench
        .feed(2, envelope(Kind::Unregister, 5, "settings"))
        .unwrap();
    expect_error(&second, 5, ErrorCode::UnknownService);
    bench
        .feed(1, envelope(Kind::Unregister, 6, "settings"))
        .unwrap();
    expect_ack(&first, 6);
    bench
        .feed(2, envelope(Kind::Register, 7, "settings"))
        .unwrap();
    expect_ack(&second, 7);
}

#[test]
fn request_is_forwarded_with_origin_and_answer_comes_back() {
    let mut bench = Bench::open();
    let caller = bench.join(1, "app.phone");
    let service = bench.join(2, "system.settingsd");
    bench
        .feed(2, envelope(Kind::Register, 1, "settings"))
        .unwrap();
    expect_ack(&service, 1);
    bench
        .feed(1, call(40, "settings", "get").with_payload(2, vec![9]))
        .unwrap();
    let forwarded = service.next().unwrap();
    assert_eq!(forwarded.kind, Kind::Request);
    assert_eq!(forwarded.origin, "app.phone");
    assert_eq!((forwarded.method.as_str(), forwarded.version), ("get", 2));
    assert_ne!(forwarded.id, 40);
    let answer = Envelope::new(Kind::Response, forwarded.id).with_payload(2, vec![1, 2]);
    bench.feed(2, answer.clone()).unwrap();
    let reply = caller.next().unwrap();
    assert_eq!(
        (reply.kind, reply.id, reply.payload),
        (Kind::Response, 40, vec![1, 2])
    );
    // A duplicated answer is discarded.
    bench.feed(2, answer).unwrap();
    assert!(caller.next().is_none());
}

#[test]
fn request_failures_are_typed() {
    let mut bench = Bench::open();
    let caller = bench.join(1, "app.phone");
    bench.feed(1, call(1, "settings", "get")).unwrap();
    expect_error(&caller, 1, ErrorCode::UnknownService);
    bench.feed(1, call(2, "settings", "bad.method")).unwrap();
    expect_error(&caller, 2, ErrorCode::InvalidName);
    bench.feed(1, call(3, "", "get")).unwrap();
    expect_error(&caller, 3, ErrorCode::InvalidName);
}

#[test]
fn busy_service_is_not_dropped_but_caller_is_told() {
    let mut bench = Bench::open();
    let caller = bench.join(1, "app.phone");
    let service = add_conn(&mut bench.state, 2, "svc", 2);
    bench.feed(2, envelope(Kind::Register, 1, "slow")).unwrap();
    bench.feed(1, call(10, "slow", "work")).unwrap();
    bench.feed(1, call(11, "slow", "work")).unwrap();
    expect_error(&caller, 11, ErrorCode::ServiceBusy);
    assert!(!service.control.is_closing());
    assert_eq!(bench.state.pending.len(), 1);
}

#[test]
fn answers_from_the_wrong_connection_are_ignored() {
    let mut bench = Bench::open();
    let caller = bench.join(1, "app.phone");
    let service = bench.join(2, "svc");
    let _intruder = bench.join(3, "app.evil");
    bench
        .feed(2, envelope(Kind::Register, 1, "settings"))
        .unwrap();
    bench.feed(1, call(5, "settings", "get")).unwrap();
    expect_ack(&service, 1);
    let forwarded = service.next().unwrap();
    bench
        .feed(3, Envelope::new(Kind::Response, forwarded.id))
        .unwrap();
    assert!(caller.next().is_none());
    bench
        .feed(2, Envelope::error(forwarded.id, ErrorCode::UnknownMethod))
        .unwrap();
    expect_error(&caller, 5, ErrorCode::UnknownMethod);
}

#[test]
fn events_reach_matching_subscribers_once_and_in_order() {
    let mut bench = Bench::open();
    let publisher = bench.join(1, "system.devd");
    let subscriber = bench.join(2, "ui");
    let other = bench.join(3, "app.x");
    bench
        .feed(2, envelope(Kind::Subscribe, 1, "devices.*"))
        .unwrap();
    bench
        .feed(2, envelope(Kind::Subscribe, 2, "devices.usb"))
        .unwrap();
    bench
        .feed(3, envelope(Kind::Subscribe, 1, "power.state"))
        .unwrap();
    expect_ack(&subscriber, 1);
    expect_ack(&subscriber, 2);
    expect_ack(&other, 1);
    for sequence in 0..3u8 {
        bench
            .feed(1, event(0, "devices.usb", 0, &[sequence]))
            .unwrap();
    }
    for sequence in 0..3u8 {
        let received = subscriber.next().unwrap();
        assert_eq!(
            (received.id, received.payload, received.flags),
            (0, vec![sequence], 0)
        );
    }
    assert!(subscriber.next().is_none());
    assert!(other.next().is_none());
    assert!(publisher.next().is_none());
}

#[test]
fn retained_values_are_replayed_to_the_new_subscription_only() {
    let mut bench = Bench::open();
    let publisher = bench.join(1, "system.power");
    let late = bench.join(2, "ui");
    bench
        .feed(1, event(8, "power.state", FLAG_RETAIN | FLAG_ACK, &[1]))
        .unwrap();
    bench
        .feed(1, event(9, "power.state", FLAG_RETAIN | FLAG_ACK, &[2]))
        .unwrap();
    expect_ack(&publisher, 8);
    expect_ack(&publisher, 9);
    bench
        .feed(2, envelope(Kind::Subscribe, 33, "power.*"))
        .unwrap();
    expect_ack(&late, 33);
    let replay = late.next().unwrap();
    assert_eq!(
        (replay.id, replay.flags, replay.payload),
        (33, FLAG_RETAIN, vec![2])
    );
    assert!(late.next().is_none());
    bench.feed(2, envelope(Kind::Unsubscribe, 33, "")).unwrap();
    expect_ack(&late, 33);
    bench.feed(1, event(0, "power.state", 0, &[3])).unwrap();
    assert!(late.next().is_none());
}

#[test]
fn limits_are_enforced() {
    let mut bench = Bench::open();
    bench.config.max_subscriptions = 1;
    bench.config.max_retained = 1;
    let client = bench.join(1, "a");
    bench.feed(1, envelope(Kind::Subscribe, 1, "x")).unwrap();
    bench.feed(1, envelope(Kind::Subscribe, 2, "y")).unwrap();
    expect_ack(&client, 1);
    expect_error(&client, 2, ErrorCode::LimitExceeded);
    // Re-sending an existing subscription id replaces it instead of adding.
    bench.feed(1, envelope(Kind::Subscribe, 1, "z")).unwrap();
    expect_ack(&client, 1);
    bench
        .feed(1, event(3, "one", FLAG_RETAIN | FLAG_ACK, &[]))
        .unwrap();
    bench
        .feed(1, event(4, "two", FLAG_RETAIN | FLAG_ACK, &[]))
        .unwrap();
    bench
        .feed(1, event(5, "one", FLAG_RETAIN | FLAG_ACK, &[]))
        .unwrap();
    expect_ack(&client, 3);
    expect_error(&client, 4, ErrorCode::LimitExceeded);
    expect_ack(&client, 5);
}

#[test]
fn default_deny_blocks_every_operation() {
    let policy = Policy::deny_all().with_rule(
        Rule::new("ui")
            .and_then(|rule| rule.subscribe("power.*"))
            .unwrap(),
    );
    let mut bench = Bench::new(policy);
    let ui = bench.join(1, "ui");
    bench.feed(1, envelope(Kind::Register, 1, "power")).unwrap();
    expect_error(&ui, 1, ErrorCode::PermissionDenied);
    bench.feed(1, call(2, "power", "sleep")).unwrap();
    expect_error(&ui, 2, ErrorCode::PermissionDenied);
    bench
        .feed(1, event(3, "power.state", FLAG_ACK, &[]))
        .unwrap();
    expect_error(&ui, 3, ErrorCode::PermissionDenied);
    bench.feed(1, event(0, "power.state", 0, &[])).unwrap();
    assert!(
        ui.next().is_none(),
        "unconfirmed publications fail silently"
    );
    bench.feed(1, envelope(Kind::Subscribe, 4, "*")).unwrap();
    expect_error(&ui, 4, ErrorCode::PermissionDenied);
    bench
        .feed(1, envelope(Kind::Subscribe, 5, "power.state"))
        .unwrap();
    expect_ack(&ui, 5);
    bench
        .feed(1, envelope(Kind::Subscribe, 6, "not a pattern"))
        .unwrap();
    expect_error(&ui, 6, ErrorCode::InvalidName);
    assert!(bench.state.retained.is_empty());
}

#[test]
fn handshake_kinds_after_the_handshake_end_the_connection() {
    let mut bench = Bench::open();
    let _client = bench.join(1, "a");
    for kind in [Kind::Hello, Kind::Welcome] {
        assert_eq!(
            bench.feed(1, Envelope::new(kind, 0)),
            Err(ErrorCode::ProtocolViolation)
        );
    }
}

#[test]
fn slow_subscriber_is_dropped_while_others_keep_receiving() {
    let mut bench = Bench::open();
    let _publisher = bench.join(1, "pub");
    let slow = add_conn(&mut bench.state, 2, "slow", 3);
    let fast = add_conn(&mut bench.state, 3, "fast", 64);
    bench.feed(2, envelope(Kind::Subscribe, 1, "*")).unwrap();
    bench.feed(3, envelope(Kind::Subscribe, 1, "*")).unwrap();
    for sequence in 0..10u8 {
        bench.feed(1, event(0, "tick", 0, &[sequence])).unwrap();
    }
    assert_eq!(slow.control.reason(), Some(ErrorCode::SlowConsumer));
    assert_eq!(bench.state.dropped_slow, 1);
    expect_ack(&fast, 1);
    for sequence in 0..10u8 {
        assert_eq!(fast.next().unwrap().payload, vec![sequence]);
    }
}

/// One identity cannot take the whole retained table, and one connection registers a bounded
/// number of services.
#[test]
fn per_identity_limits() {
    let mut bench = Bench::open();
    bench.config.max_retained = 10;
    bench.config.max_retained_per_identity = 2;
    bench.config.max_services_per_connection = 1;
    let greedy = bench.join(1, "greedy");
    let other = bench.join(2, "other");
    for (id, topic) in [(1, "g.one"), (2, "g.two"), (3, "g.three")] {
        bench
            .feed(1, event(id, topic, FLAG_RETAIN | FLAG_ACK, &[]))
            .unwrap();
    }
    expect_ack(&greedy, 1);
    expect_ack(&greedy, 2);
    expect_error(&greedy, 3, ErrorCode::LimitExceeded);
    // its own topics may still change
    bench
        .feed(1, event(4, "g.one", FLAG_RETAIN | FLAG_ACK, &[1]))
        .unwrap();
    expect_ack(&greedy, 4);
    // another identity still has room
    bench
        .feed(2, event(5, "o.state", FLAG_RETAIN | FLAG_ACK, &[]))
        .unwrap();
    expect_ack(&other, 5);
    bench.feed(1, envelope(Kind::Register, 6, "first")).unwrap();
    expect_ack(&greedy, 6);
    bench
        .feed(1, envelope(Kind::Register, 7, "second"))
        .unwrap();
    expect_error(&greedy, 7, ErrorCode::LimitExceeded);
}
