//! Client life cycle: reconnection, re-announcement, closing.

mod common;

use common::{client, eventually, open_broker, socket_path, start_at, token_broker, QUIET, WAIT};
use std::thread;
use std::time::Duration;
use ultar_bus::messages::power::{PowerProfile, PowerSource, PowerState, ThermalLevel};
use ultar_bus::{
    BrokerConfig, BusError, Client, ClientOptions, ErrorCode, Policy, Rule, TokenIdentity,
};

fn power(percent: u8) -> PowerState {
    PowerState {
        source: PowerSource::External,
        battery_percent: Some(percent),
        charging: true,
        profile: PowerProfile::Performance,
        thermal: ThermalLevel::Normal,
    }
}

fn fast_reconnect(name: &str) -> ClientOptions {
    ClientOptions {
        reconnect_min: Duration::from_millis(10),
        reconnect_max: Duration::from_millis(50),
        ..ClientOptions::named(name)
    }
}

#[test]
fn client_reconnects_and_reannounces_after_a_broker_restart() {
    let path = socket_path("restart");
    let broker = start_at(&path, Policy::allow_all(), BrokerConfig::default());
    let node = Client::connect(&path, fast_reconnect("node")).unwrap();
    let state = node.subscribe::<PowerState>().unwrap();
    let endpoint = node.register("echo").unwrap();
    let worker = thread::spawn(move || {
        endpoint.run(|request| {
            let value: u32 = request.decode_raw().unwrap_or(0);
            let _ = request.reply_raw(&(value + 1));
        });
    });
    let other = Client::connect(&path, fast_reconnect("other")).unwrap();
    assert_eq!(
        other
            .request::<_, u32>("echo", "bump", &1u32, WAIT)
            .unwrap(),
        2
    );

    broker.shutdown();
    eventually("both notice the loss", || {
        !node.is_connected() && !other.is_connected()
    });
    let offline = other.request::<_, u32>("echo", "bump", &1u32, WAIT);
    assert!(
        matches!(offline, Err(BusError::NotConnected)),
        "{offline:?}"
    );
    assert!(matches!(
        node.publish(&power(1)),
        Err(BusError::NotConnected)
    ));
    assert!(!node.wait_connected(QUIET));

    let broker = start_at(&path, Policy::allow_all(), BrokerConfig::default());
    assert!(node.wait_connected(WAIT));
    assert!(other.wait_connected(WAIT));
    eventually("service and subscription are back", || {
        let stats = broker.stats();
        stats.services == 1 && stats.subscriptions == 1
    });
    assert_eq!(
        other
            .request::<_, u32>("echo", "bump", &41u32, WAIT)
            .unwrap(),
        42
    );
    other.publish(&power(77)).unwrap();
    assert_eq!(state.recv_timeout(WAIT).unwrap(), power(77));

    node.close();
    worker.join().unwrap();
}

#[test]
fn retained_state_is_delivered_again_after_reconnecting() {
    let path = socket_path("restart-retained");
    let broker = start_at(&path, Policy::allow_all(), BrokerConfig::default());
    let watcher = Client::connect(&path, fast_reconnect("watcher")).unwrap();
    let state = watcher.subscribe::<PowerState>().unwrap();
    broker.shutdown();
    eventually("loss noticed", || !watcher.is_connected());

    let _broker = start_at(&path, Policy::allow_all(), BrokerConfig::default());
    // The publisher comes back first and states the current value …
    let publisher = client(&path, "powerd");
    publisher.publish(&power(33)).unwrap();
    // … and the watcher receives it, live or as retained replay.
    assert_eq!(state.recv_timeout(WAIT).unwrap(), power(33));
}

#[test]
fn pending_call_fails_when_the_broker_goes_away() {
    let (broker, path) = open_broker("pending-loss");
    let service = Client::connect(&path, fast_reconnect("service")).unwrap();
    let _endpoint = service.register("stuck").unwrap();
    let app = Client::connect(&path, fast_reconnect("app")).unwrap();
    let caller = thread::spawn(move || app.request::<_, ()>("stuck", "wait", &(), WAIT));
    eventually("request pending", || broker.stats().pending_requests == 1);
    broker.shutdown();
    let result = caller.join().unwrap();
    assert!(matches!(result, Err(BusError::Closed)), "{result:?}");
}

#[test]
fn without_reconnect_the_client_ends_with_the_connection() {
    let (broker, path) = open_broker("no-reconnect");
    let options = ClientOptions {
        reconnect: false,
        ..ClientOptions::named("once")
    };
    let app = Client::connect(&path, options).unwrap();
    let stream = app.subscribe_pattern("*").unwrap();
    broker.shutdown();
    assert!(matches!(stream.recv_timeout(WAIT), Err(BusError::Closed)));
    eventually("closed", || {
        matches!(
            app.request::<_, ()>("a", "b", &(), QUIET),
            Err(BusError::Closed)
        )
    });
    assert!(!app.is_connected());
}

#[test]
fn close_releases_everything_that_waits() {
    let (broker, path) = open_broker("close");
    let service = client(&path, "service");
    let _keep_unanswered = service.register("stuck").unwrap();
    let app = client(&path, "app");
    let stream = app.subscribe_pattern("*").unwrap();
    let endpoint = app.register("mine").unwrap();
    let caller = {
        let app = app.clone();
        thread::spawn(move || app.request::<_, ()>("stuck", "wait", &(), WAIT))
    };
    let listener = thread::spawn(move || stream.recv());
    let server = thread::spawn(move || endpoint.recv().map(|_| ()));
    eventually("request pending", || broker.stats().pending_requests == 1);
    app.close();
    assert!(matches!(caller.join().unwrap(), Err(BusError::Closed)));
    assert!(matches!(listener.join().unwrap(), Err(BusError::Closed)));
    assert!(matches!(server.join().unwrap(), Err(BusError::Closed)));
    assert!(matches!(app.subscribe_pattern("*"), Err(BusError::Closed)));
    assert!(matches!(app.register("x"), Err(BusError::Closed)));
    eventually("connection removed", || broker.stats().connections == 1);
}

#[test]
fn dropping_the_last_handle_closes_the_connection() {
    let (broker, path) = open_broker("drop");
    let app = client(&path, "app");
    let clone = app.clone();
    let stream = app.subscribe_pattern("*").unwrap();
    drop(app);
    assert_eq!(broker.stats().connections, 1);
    drop(clone);
    eventually("connection removed", || broker.stats().connections == 0);
    assert!(matches!(stream.recv_timeout(WAIT), Err(BusError::Closed)));
}

#[test]
fn connect_can_wait_for_a_broker_that_starts_later() {
    let path = socket_path("late-broker");
    let missing = Client::connect(&path, ClientOptions::named("impatient"));
    assert!(matches!(missing, Err(BusError::Io(_))));
    let starter = {
        let path = path.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            start_at(&path, Policy::allow_all(), BrokerConfig::default())
        })
    };
    let options = ClientOptions {
        connect_retry_for: WAIT,
        ..ClientOptions::named("patient")
    };
    let app = Client::connect(&path, options).unwrap();
    assert!(app.is_connected());
    assert!(app.wait_connected(QUIET));
    let _broker = starter.join().unwrap();
}

#[test]
fn reannouncement_refused_by_a_new_policy_ends_the_subscription() {
    let path = socket_path("reannounce-denied");
    let auth = || {
        std::sync::Arc::new(TokenIdentity::new().with_token("app.phone", common::id("app.phone")))
    };
    let broker = ultar_bus::Broker::builder(Policy::allow_all())
        .listen(&path, auth())
        .start()
        .unwrap();
    let options = ClientOptions {
        token: Some("app.phone".into()),
        ..fast_reconnect("phone")
    };
    let phone = Client::connect(&path, options).unwrap();
    let allowed = phone.subscribe_pattern("shell.mode").unwrap();
    let revoked = phone.subscribe_pattern("devices.*").unwrap();
    let endpoint = phone.register("dialer").unwrap();
    broker.shutdown();
    eventually("loss noticed", || !phone.is_connected());

    // The restarted broker only lets the app follow the shell mode.
    let stricter = Policy::deny_all().with_rule(
        Rule::new("app.phone")
            .and_then(|rule| rule.subscribe("shell.mode"))
            .and_then(|rule| rule.publish("shell.mode"))
            .unwrap(),
    );
    let broker = ultar_bus::Broker::builder(stricter)
        .listen(&path, auth())
        .start()
        .unwrap();
    assert!(phone.wait_connected(WAIT));
    let refused = revoked.recv_timeout(WAIT);
    assert!(
        matches!(refused, Err(BusError::Remote(ErrorCode::PermissionDenied))),
        "{refused:?}"
    );
    assert!(matches!(revoked.recv_timeout(WAIT), Err(BusError::Closed)));
    assert!(matches!(endpoint.recv_timeout(WAIT), Err(BusError::Closed)));
    phone
        .publish_raw("shell.mode", 1, false, vec![1], true)
        .unwrap();
    assert_eq!(allowed.recv_timeout(WAIT).unwrap().payload, vec![1]);
    assert_eq!(broker.stats().subscriptions, 1);
}

#[test]
fn wrong_token_after_restart_stops_the_client_for_good() {
    let (broker, path) = token_broker("auth-revoked", Policy::allow_all());
    let options = ClientOptions {
        token: Some("app.phone".into()),
        ..fast_reconnect("phone")
    };
    let phone = Client::connect(&path, options).unwrap();
    let stream = phone.subscribe_pattern("*").unwrap();
    broker.shutdown();
    // The new broker knows no tokens at all.
    let _broker = ultar_bus::Broker::builder(Policy::allow_all())
        .listen(&path, std::sync::Arc::new(TokenIdentity::new()))
        .start()
        .unwrap();
    assert!(matches!(stream.recv_timeout(WAIT), Err(BusError::Closed)));
    assert!(!phone.wait_connected(QUIET));
}
