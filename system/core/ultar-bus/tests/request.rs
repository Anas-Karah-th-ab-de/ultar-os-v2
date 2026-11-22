//! Request/response over real sockets.

mod common;

use common::{client, client_with_token, eventually, open_broker, token_broker, WAIT};
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::{Duration, Instant};
use ultar_bus::messages::settings::{Get, GetReply, Set, SetReply, SettingValue};
use ultar_bus::{BusError, Call, Client, ErrorCode, Message, Policy, ServiceEndpoint};

/// Minimal settings service: `get` answers the key length, `set` says changed.
fn spawn_settings(client: &Client) -> thread::JoinHandle<()> {
    let endpoint = client.register("settings").unwrap();
    thread::spawn(move || {
        endpoint.run(|request| {
            let outcome = match request.method() {
                "get" => request.respond::<Get>(|get| {
                    Ok(GetReply {
                        value: Some(SettingValue::Integer(get.key.len() as i64)),
                    })
                }),
                "set" => request.respond::<Set>(|_| Ok(SetReply { changed: true })),
                "boom" => request.fail(ErrorCode::Service(42)),
                _ => request.fail(ErrorCode::UnknownMethod),
            };
            let _ = outcome;
        });
    })
}

#[test]
fn typed_call_round_trip() {
    let (_broker, path) = open_broker("typed-call");
    let service = client(&path, "settingsd");
    let _worker = spawn_settings(&service);
    let app = client(&path, "app");
    let reply = app
        .call(
            &Get {
                key: "display.scale".into(),
            },
            WAIT,
        )
        .unwrap();
    assert_eq!(reply.value, Some(SettingValue::Integer(13)));
    let reply = app
        .call(
            &Set {
                key: "a".into(),
                value: SettingValue::Flag(true),
            },
            WAIT,
        )
        .unwrap();
    assert!(reply.changed);
}

#[test]
fn untyped_request_round_trip_and_caller_identity() {
    let (_broker, path) = token_broker("raw-request", Policy::allow_all());
    let service = client_with_token(&path, "system.settingsd").unwrap();
    assert_eq!(service.identity(), "system.settingsd");
    let endpoint = service.register("echo").unwrap();
    assert_eq!(endpoint.name(), "echo");
    let worker = thread::spawn(move || {
        let request = endpoint.recv().unwrap();
        assert_eq!(request.method(), "shout");
        assert_eq!(request.version(), 0);
        assert!(!request.payload().is_empty());
        let caller = request.caller().to_owned();
        let text: String = request.decode_raw().unwrap();
        request.reply_raw(&(text.to_uppercase(), caller)).unwrap();
    });
    let app = client_with_token(&path, "app.phone").unwrap();
    let (text, caller): (String, String) = app.request("echo", "shout", &"salam", WAIT).unwrap();
    assert_eq!(text, "SALAM");
    assert_eq!(caller, "app.phone");
    worker.join().unwrap();
}

#[test]
fn unknown_service_and_unknown_method() {
    let (_broker, path) = open_broker("unknown");
    let service = client(&path, "settingsd");
    let _worker = spawn_settings(&service);
    let app = client(&path, "app");
    let missing: Result<(), _> = app.request("nowhere", "get", &(), WAIT);
    assert!(matches!(
        missing,
        Err(BusError::Remote(ErrorCode::UnknownService))
    ));
    let method: Result<(), _> = app.request("settings", "frobnicate", &(), WAIT);
    assert!(matches!(
        method,
        Err(BusError::Remote(ErrorCode::UnknownMethod))
    ));
    let failed: Result<(), _> = app.request("settings", "boom", &(), WAIT);
    assert!(matches!(
        failed,
        Err(BusError::Remote(ErrorCode::Service(42)))
    ));
}

#[test]
fn invalid_names_fail_before_anything_is_sent() {
    let (_broker, path) = open_broker("names");
    let app = client(&path, "app");
    for (service, method) in [("Bad", "get"), ("ok", "a.b"), ("", "get"), ("ok", "")] {
        let result: Result<(), _> = app.request(service, method, &(), WAIT);
        assert!(
            matches!(result, Err(BusError::InvalidName)),
            "{service}/{method}"
        );
    }
    assert!(matches!(
        app.register("Not Valid"),
        Err(BusError::InvalidName)
    ));
}

#[test]
fn request_times_out_and_late_answer_is_discarded() {
    let (_broker, path) = open_broker("timeout");
    let service = client(&path, "slow");
    let endpoint = service.register("slow").unwrap();
    let app = client(&path, "app");
    let started = Instant::now();
    let result: Result<u32, _> = app.request("slow", "work", &1u32, Duration::from_millis(100));
    assert!(matches!(result, Err(BusError::Timeout)));
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert!(started.elapsed() < Duration::from_secs(5));
    // The late answer must not confuse the next call.
    endpoint.recv().unwrap().reply_raw(&111u32).unwrap();
    let worker = thread::spawn(move || endpoint.recv().unwrap().reply_raw(&222u32).unwrap());
    let second: u32 = app.request("slow", "work", &2u32, WAIT).unwrap();
    assert_eq!(second, 222);
    worker.join().unwrap();
}

#[test]
fn dropped_request_answers_no_reply() {
    let (_broker, path) = open_broker("noreply");
    let service = client(&path, "svc");
    let endpoint = service.register("lazy").unwrap();
    let worker = thread::spawn(move || drop(endpoint.recv_timeout(WAIT).unwrap()));
    let app = client(&path, "app");
    let result: Result<(), _> = app.request("lazy", "work", &(), WAIT);
    assert!(matches!(result, Err(BusError::Remote(ErrorCode::NoReply))));
    worker.join().unwrap();
}

#[test]
fn service_that_disconnects_fails_waiting_callers() {
    let (broker, path) = open_broker("gone");
    let service = client(&path, "svc");
    let endpoint = service.register("fragile").unwrap();
    let app = client(&path, "app");
    let caller = thread::spawn(move || app.request::<_, ()>("fragile", "work", &(), WAIT));
    eventually("request reached the broker", || {
        broker.stats().pending_requests == 1
    });
    // Keep the request unanswered in the inbox and cut the connection.
    service.close();
    let result = caller.join().unwrap();
    assert!(
        matches!(result, Err(BusError::Remote(ErrorCode::ServiceGone))),
        "{result:?}"
    );
    eventually("service forgotten", || broker.stats().services == 0);
    assert_eq!(broker.stats().pending_requests, 0);
    drop(endpoint);
}

#[test]
fn service_names_are_exclusive_until_released() {
    let (broker, path) = open_broker("exclusive");
    let first = client(&path, "first");
    let second = client(&path, "second");
    let endpoint: ServiceEndpoint = first.register("power").unwrap();
    assert!(matches!(
        second.register("power"),
        Err(BusError::Remote(ErrorCode::NameTaken))
    ));
    assert!(matches!(
        first.register("power"),
        Err(BusError::Remote(ErrorCode::NameTaken))
    ));
    assert!(endpoint.try_recv().unwrap().is_none());
    drop(endpoint);
    eventually("name released", || broker.stats().services == 0);
    let taken_over = second.register("power").unwrap();
    assert!(matches!(
        taken_over.recv_timeout(Duration::from_millis(20)),
        Err(BusError::Timeout)
    ));
}

/// Same call as [`Get`] but built against a newer contract.
#[derive(Serialize, Deserialize)]
struct GetV2 {
    key: String,
    scope: u8,
}

impl Message for GetV2 {
    const VERSION: u16 = 2;
}

impl Call for GetV2 {
    const SERVICE: &'static str = "settings";
    const METHOD: &'static str = "get";
    type Reply = GetReply;
}

/// A call whose reply this side expects in a newer version.
#[derive(Serialize, Deserialize)]
struct NewReply;

impl Message for NewReply {
    const VERSION: u16 = 9;
}

#[derive(Serialize, Deserialize)]
struct SetExpectingNewReply {
    key: String,
    value: SettingValue,
}

impl Message for SetExpectingNewReply {
    const VERSION: u16 = 1;
}

impl Call for SetExpectingNewReply {
    const SERVICE: &'static str = "settings";
    const METHOD: &'static str = "set";
    type Reply = NewReply;
}

#[test]
fn message_version_mismatch_is_a_typed_error_in_both_directions() {
    let (_broker, path) = open_broker("msg-version");
    let service = client(&path, "settingsd");
    let _worker = spawn_settings(&service);
    let app = client(&path, "app");
    let newer = app.call(
        &GetV2 {
            key: "k".into(),
            scope: 1,
        },
        WAIT,
    );
    assert!(matches!(
        newer,
        Err(BusError::Remote(ErrorCode::VersionMismatch))
    ));
    let call = SetExpectingNewReply {
        key: "k".into(),
        value: SettingValue::Flag(false),
    };
    let reply = app.call(&call, WAIT);
    assert!(matches!(
        reply,
        Err(BusError::MessageVersion {
            expected: 9,
            found: 1
        })
    ));
    // The connection is still fine.
    assert!(app.call(&Get { key: "abc".into() }, WAIT).is_ok());
}

#[test]
fn undecodable_payload_is_reported_as_bad_payload() {
    let (_broker, path) = open_broker("bad-payload");
    let service = client(&path, "settingsd");
    let endpoint = service.register("settings").unwrap();
    let worker = thread::spawn(move || {
        let request = endpoint.recv().unwrap();
        // The caller claims version 1 of `Set` but sends a bare integer.
        assert!(matches!(
            request.decode::<Get>(),
            Err(BusError::Remote(ErrorCode::UnknownMethod))
        ));
        request
            .respond::<Set>(|_| Ok(SetReply { changed: true }))
            .unwrap();
    });
    let app = client(&path, "app");

    #[derive(Serialize, Deserialize)]
    struct Garbage(u8);
    impl Message for Garbage {
        const VERSION: u16 = 1;
    }
    impl Call for Garbage {
        const SERVICE: &'static str = "settings";
        const METHOD: &'static str = "set";
        type Reply = SetReply;
    }
    let result = app.call(&Garbage(200), WAIT);
    assert!(
        matches!(result, Err(BusError::Remote(ErrorCode::BadPayload))),
        "{result:?}"
    );
    worker.join().unwrap();
}

#[test]
fn a_handler_may_call_another_service_on_the_same_connection() {
    let (_broker, path) = open_broker("nested");
    let daemon = client(&path, "daemon");
    let inner = daemon.register("inner").unwrap();
    let outer = daemon.register("outer").unwrap();
    let inner_worker = thread::spawn(move || {
        let request = inner.recv().unwrap();
        let value: u32 = request.decode_raw().unwrap();
        request.reply_raw(&(value + 1)).unwrap();
    });
    let nested = daemon.clone();
    let outer_worker = thread::spawn(move || {
        let request = outer.recv().unwrap();
        let value: u32 = request.decode_raw().unwrap();
        let bumped: u32 = nested.request("inner", "bump", &value, WAIT).unwrap();
        request.reply_raw(&(bumped * 10)).unwrap();
    });
    let app = client(&path, "app");
    let answer: u32 = app.request("outer", "run", &4u32, WAIT).unwrap();
    assert_eq!(answer, 50);
    inner_worker.join().unwrap();
    outer_worker.join().unwrap();
}

#[test]
fn many_threads_share_one_client() {
    let (_broker, path) = open_broker("shared");
    let service = client(&path, "settingsd");
    let _worker = spawn_settings(&service);
    let app = client(&path, "app");
    let callers: Vec<_> = (0..8usize)
        .map(|index| {
            let app = app.clone();
            thread::spawn(move || {
                for round in 0..50usize {
                    let key = "k".repeat(index * 50 + round + 1);
                    let reply = app.call(&Get { key: key.clone() }, WAIT).unwrap();
                    assert_eq!(reply.value, Some(SettingValue::Integer(key.len() as i64)));
                }
            })
        })
        .collect();
    for caller in callers {
        caller.join().unwrap();
    }
}
