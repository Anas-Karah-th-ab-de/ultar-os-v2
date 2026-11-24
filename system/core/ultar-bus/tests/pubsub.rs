//! Publish/subscribe over real sockets.

mod common;

use common::{client, eventually, open_broker, QUIET, WAIT};
use std::path::Path;
use std::thread;
use ultar_bus::messages::devices::{
    Availability, Capabilities, Capability, DriverPhase, DriverState,
};
use ultar_bus::messages::notify::{Body, Category, Post, Posted, Urgency};
use ultar_bus::messages::power::{PowerProfile, PowerSource, PowerState, ThermalLevel};
use ultar_bus::messages::shell::{Mode, ShellMode};
use ultar_bus::{encode, BusError, Client, ClientOptions, Message, Text, Topic};

fn power(percent: u8) -> PowerState {
    PowerState {
        source: PowerSource::Battery,
        battery_percent: Some(percent),
        charging: false,
        profile: PowerProfile::Balanced,
        thermal: ThermalLevel::Normal,
    }
}

fn driver(device: &str) -> DriverState {
    DriverState {
        device: device.into(),
        driver: "demo".into(),
        phase: DriverPhase::Active,
        reason: None,
    }
}

fn capabilities(revision: u64) -> Capabilities {
    Capabilities {
        revision,
        capabilities: vec![Capability {
            id: "modem".into(),
            availability: Availability::Available,
            note: None,
        }],
    }
}

fn sequence_event(publisher: &Client, topic: &str, sequence: u32) {
    publisher
        .publish_raw(topic, 1, false, encode(&sequence).unwrap(), false)
        .unwrap();
}

#[test]
fn typed_event_reaches_subscriber() {
    let (_broker, path) = open_broker("typed-event");
    let shell = client(&path, "shell");
    let notifications = shell.subscribe::<Posted>().unwrap();
    assert!(notifications.try_recv().unwrap().is_none());
    let service = client(&path, "notifyd");
    let posted = Posted {
        id: 1,
        timestamp_ms: 5,
        post: Post {
            urgency: Urgency::High,
            category: Category::Call,
            app_id: "tech.code-craft.phone".into(),
            title: Text::key("notify.call.incoming"),
            body: Body::Empty,
            actions: Vec::new(),
        },
        sender: "app.phone".into(),
    };
    service.publish(&posted).unwrap();
    assert_eq!(notifications.recv_timeout(WAIT).unwrap(), posted);
    service.publish_unconfirmed(&posted).unwrap();
    assert_eq!(notifications.recv().unwrap(), posted);
}

#[test]
fn prefix_subscription_receives_the_whole_branch_only() {
    let (_broker, path) = open_broker("prefix");
    let watcher = client(&path, "watcher");
    let devices = watcher.subscribe_pattern("devices.*").unwrap();
    let everything = watcher.subscribe_pattern("*").unwrap();
    let exact = watcher.subscribe_pattern("devices.driver_state").unwrap();
    let publisher = client(&path, "devd");
    publisher.publish(&power(50)).unwrap();
    publisher.publish(&driver("usb-1")).unwrap();
    publisher.publish(&capabilities(1)).unwrap();

    let first = devices.recv_timeout(WAIT).unwrap();
    assert_eq!(first.topic, "devices.driver_state");
    assert!(!first.retained);
    assert_eq!(first.decode::<DriverState>().unwrap(), driver("usb-1"));
    assert!(matches!(
        first.decode::<PowerState>(),
        Err(BusError::InvalidName)
    ));
    assert_eq!(
        devices.recv_timeout(WAIT).unwrap().topic,
        "devices.capabilities"
    );
    assert!(matches!(
        devices.recv_timeout(QUIET),
        Err(BusError::Timeout)
    ));

    let topics: Vec<String> = (0..3)
        .map(|_| everything.recv_timeout(WAIT).unwrap().topic)
        .collect();
    assert_eq!(
        topics,
        [
            "power.state",
            "devices.driver_state",
            "devices.capabilities"
        ]
    );
    assert_eq!(
        exact.recv_timeout(WAIT).unwrap().topic,
        "devices.driver_state"
    );
    assert!(exact.try_recv().unwrap().is_none());
}

#[test]
fn late_subscriber_gets_the_retained_value_immediately() {
    let (broker, path) = open_broker("retained");
    let publisher = client(&path, "powerd");
    publisher.publish(&power(90)).unwrap();
    publisher.publish(&power(80)).unwrap();
    publisher.publish(&driver("not-retained")).unwrap();
    publisher
        .publish(&ShellMode {
            mode: Mode::Desktop,
        })
        .unwrap();
    assert_eq!(broker.stats().retained, 2);

    let late = client(&path, "late");
    let state = late.subscribe::<PowerState>().unwrap();
    assert_eq!(state.recv_timeout(WAIT).unwrap(), power(80));
    assert!(matches!(state.recv_timeout(QUIET), Err(BusError::Timeout)));

    let all = late.subscribe_pattern("*").unwrap();
    let mut replayed: Vec<String> = (0..2)
        .map(|_| {
            let event = all.recv_timeout(WAIT).unwrap();
            assert!(event.retained);
            event.topic
        })
        .collect();
    replayed.sort();
    assert_eq!(replayed, ["power.state", "shell.mode"]);
    assert!(matches!(all.recv_timeout(QUIET), Err(BusError::Timeout)));
    // The replay for the second subscription is not duplicated to the first.
    assert!(state.try_recv().unwrap().is_none());

    let mode = late.subscribe::<ShellMode>().unwrap().into_raw();
    assert!(mode.recv_timeout(WAIT).unwrap().retained);
    // Live updates still arrive after the replay, exactly once each.
    publisher.publish(&power(70)).unwrap();
    assert_eq!(state.recv_timeout(WAIT).unwrap(), power(70));
    assert!(!all.recv_timeout(WAIT).unwrap().retained);
}

#[test]
fn retained_value_survives_its_publisher() {
    let (_broker, path) = open_broker("retained-survives");
    {
        let publisher = client(&path, "devd");
        publisher.publish(&capabilities(7)).unwrap();
    }
    let late = client(&path, "settings-app");
    let caps = late.subscribe::<Capabilities>().unwrap();
    let current = caps.recv_timeout(WAIT).unwrap();
    assert_eq!(current.revision, 7);
    assert!(current.has("modem"));
}

#[test]
fn events_of_a_topic_keep_their_order() {
    let (_broker, path) = open_broker("order");
    let options = ClientOptions {
        buffer: 4096,
        ..ClientOptions::named("subscriber")
    };
    let subscriber = Client::connect(&path, options).unwrap();
    let stream = subscriber.subscribe_pattern("load.sequence").unwrap();
    let publisher = client(&path, "publisher");
    for sequence in 0..2000u32 {
        sequence_event(&publisher, "load.sequence", sequence);
    }
    for expected in 0..2000u32 {
        let event = stream.recv_timeout(WAIT).unwrap();
        assert_eq!(ultar_bus::decode::<u32>(&event.payload).unwrap(), expected);
    }
}

fn collect(path: &Path, count: usize) -> thread::JoinHandle<Vec<Vec<u8>>> {
    let options = ClientOptions {
        buffer: 4096,
        ..ClientOptions::named("collector")
    };
    let subscriber = Client::connect(path, options).unwrap();
    let stream = subscriber.subscribe_pattern("race.topic").unwrap();
    thread::spawn(move || {
        let _keep = subscriber;
        (0..count)
            .map(|_| stream.recv_timeout(WAIT).unwrap().payload)
            .collect()
    })
}

#[test]
fn all_subscribers_see_concurrent_publishers_in_the_same_order() {
    let (_broker, path) = open_broker("same-order");
    let first = collect(&path, 600);
    let second = collect(&path, 600);
    let publishers: Vec<_> = (0..3u8)
        .map(|publisher_id| {
            let publisher = client(&path, "publisher");
            thread::spawn(move || {
                for sequence in 0..200u8 {
                    let payload = vec![publisher_id, sequence];
                    publisher
                        .publish_raw("race.topic", 1, false, payload, false)
                        .unwrap();
                }
                // Confirmed publish: everything before it is processed.
                publisher
                    .publish_raw("race.done", 1, false, Vec::new(), true)
                    .unwrap();
            })
        })
        .collect();
    for publisher in publishers {
        publisher.join().unwrap();
    }
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    assert_eq!(first, second);
    for publisher_id in 0..3u8 {
        let own: Vec<u8> = first
            .iter()
            .filter(|p| p[0] == publisher_id)
            .map(|p| p[1])
            .collect();
        assert_eq!(own, (0..200u8).collect::<Vec<_>>());
    }
}

#[test]
fn dropping_a_subscription_unsubscribes() {
    let (broker, path) = open_broker("unsubscribe");
    let subscriber = client(&path, "subscriber");
    let kept = subscriber.subscribe::<PowerState>().unwrap();
    let dropped = subscriber.subscribe_pattern("power.*").unwrap();
    assert_eq!(broker.stats().subscriptions, 2);
    drop(dropped);
    eventually("one subscription left", || {
        broker.stats().subscriptions == 1
    });
    let publisher = client(&path, "powerd");
    publisher.publish(&power(10)).unwrap();
    assert_eq!(kept.recv_timeout(WAIT).unwrap(), power(10));
    assert!(matches!(kept.recv_timeout(QUIET), Err(BusError::Timeout)));
    drop(subscriber);
    eventually("connection gone", || broker.stats().subscriptions == 0);
}

#[test]
fn full_local_buffer_reports_lag_and_then_continues() {
    let (_broker, path) = open_broker("lag");
    let options = ClientOptions {
        buffer: 4,
        ..ClientOptions::named("sleepy")
    };
    let sleepy = Client::connect(&path, options).unwrap();
    let stream = sleepy.subscribe_pattern("lag.topic").unwrap();
    let publisher = client(&path, "publisher");
    for sequence in 0..10u32 {
        sequence_event(&publisher, "lag.topic", sequence);
    }
    publisher
        .publish_raw("lag.done", 1, false, Vec::new(), true)
        .unwrap();
    // Make sure the subscriber's reader has seen all ten events.
    let marker = sleepy.subscribe_pattern("lag.marker").unwrap();
    publisher
        .publish_raw("lag.marker", 1, false, Vec::new(), true)
        .unwrap();
    marker.recv_timeout(WAIT).unwrap();

    assert!(matches!(
        stream.recv_timeout(WAIT),
        Err(BusError::Lagged(6))
    ));
    for expected in 0..4u32 {
        let event = stream.recv_timeout(WAIT).unwrap();
        assert_eq!(ultar_bus::decode::<u32>(&event.payload).unwrap(), expected);
    }
    sequence_event(&publisher, "lag.topic", 99);
    let event = stream.recv_timeout(WAIT).unwrap();
    assert_eq!(ultar_bus::decode::<u32>(&event.payload).unwrap(), 99);
}

#[test]
fn wrong_contract_version_is_reported_to_the_typed_subscriber() {
    let (_broker, path) = open_broker("topic-version");
    let subscriber = client(&path, "subscriber");
    let typed = subscriber.subscribe::<PowerState>().unwrap();
    let publisher = client(&path, "future-powerd");
    let future_version = PowerState::VERSION + 1;
    let payload = encode(&power(1)).unwrap();
    publisher
        .publish_raw(PowerState::TOPIC, future_version, false, payload, true)
        .unwrap();
    let result = typed.recv_timeout(WAIT);
    assert!(
        matches!(result, Err(BusError::MessageVersion { found, .. }) if found == future_version)
    );
    publisher.publish(&power(2)).unwrap();
    assert_eq!(typed.recv_timeout(WAIT).unwrap(), power(2));
}

#[test]
fn invalid_topics_and_patterns_are_rejected_locally() {
    let (_broker, path) = open_broker("invalid-topic");
    let app = client(&path, "app");
    assert!(matches!(
        app.subscribe_pattern("a.*.b"),
        Err(BusError::InvalidName)
    ));
    assert!(matches!(
        app.subscribe_pattern(""),
        Err(BusError::InvalidName)
    ));
    let result = app.publish_raw("devices.*", 1, false, Vec::new(), true);
    assert!(matches!(result, Err(BusError::InvalidName)));
}

#[test]
fn publisher_receives_its_own_events_when_subscribed() {
    let (_broker, path) = open_broker("self");
    let node = client(&path, "node");
    let own = node.subscribe::<ShellMode>().unwrap();
    node.publish(&ShellMode {
        mode: Mode::Desktop,
    })
    .unwrap();
    assert_eq!(own.recv_timeout(WAIT).unwrap().mode, Mode::Desktop);
}
