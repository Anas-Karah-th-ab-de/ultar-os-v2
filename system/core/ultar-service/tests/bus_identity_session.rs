//! Peer identity and the bus session against a real in-process broker.

mod common;

use common::{temp_dir, WAIT};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::{
    Broker, BusError, Client, ClientOptions, ErrorCode, FixedIdentity, Identity, Message, Policy,
    Topic,
};
use ultar_service::session::MAX_RETAINED_TOPICS;
use ultar_service::{effective_uid, BusSession, Secret, SessionOptions, UidMap};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
struct Mode {
    night: bool,
}

impl Message for Mode {
    const VERSION: u16 = 3;
}

impl Topic for Mode {
    const TOPIC: &'static str = "test.mode";
    const RETAINED: bool = true;
}

fn id(name: &str) -> Identity {
    Identity::new(name).unwrap()
}

fn open_broker(socket: &std::path::Path) -> Broker {
    Broker::builder(Policy::allow_all())
        .listen(socket, Arc::new(FixedIdentity::new(id("test.peer"))))
        .start()
        .unwrap()
}

#[test]
fn the_broker_identifies_this_process_by_its_kernel_credentials() {
    let dir = temp_dir("cred");
    let socket = dir.join("bus.sock");
    let map = UidMap::new().with(effective_uid(), id("system.me"));
    let broker = Broker::builder(Policy::allow_all())
        .listen(&socket, Arc::new(map.into_peer_identity()))
        .start()
        .unwrap();
    let client = Client::connect(&socket, ClientOptions::named("anything")).unwrap();
    assert_eq!(client.identity(), "system.me");
    drop(broker);

    let stranger = UidMap::new().with(effective_uid().wrapping_add(7), id("system.me"));
    let broker = Broker::builder(Policy::allow_all())
        .listen(&socket, Arc::new(stranger.into_peer_identity()))
        .start()
        .unwrap();
    let refused = Client::connect(&socket, ClientOptions::named("anything"));
    assert!(matches!(refused, Err(BusError::AuthFailed)));
    // Default deny: an empty map refuses everybody.
    drop(broker);
    let broker = Broker::builder(Policy::allow_all())
        .listen(&socket, Arc::new(UidMap::new().into_peer_identity()))
        .start()
        .unwrap();
    assert!(matches!(
        Client::connect(&socket, ClientOptions::named("x")),
        Err(BusError::AuthFailed)
    ));
    drop(broker);
}

#[test]
fn a_session_waits_for_the_broker_and_gives_up_after_its_patience() {
    let dir = temp_dir("wait");
    let socket = dir.join("bus.sock");
    let mut impatient = SessionOptions::named("t");
    impatient.connect_wait = Duration::ZERO;
    assert!(matches!(
        BusSession::connect(&socket, impatient),
        Err(BusError::Io(_))
    ));
    // The broker appears while the session is retrying.
    let starter = {
        let socket = socket.clone();
        std::thread::spawn(move || open_broker(&socket))
    };
    let mut patient = SessionOptions::named("t");
    patient.connect_wait = WAIT;
    patient.backoff_min = Duration::from_millis(1);
    patient.backoff_max = Duration::from_millis(4);
    let session = BusSession::connect(&socket, patient).unwrap();
    assert!(session.client().is_connected());
    assert_eq!(session.client().generation(), 1);
    drop(starter.join().unwrap());
}

#[test]
fn retained_values_return_after_an_in_process_broker_restart() {
    let dir = temp_dir("restart");
    let socket = dir.join("bus.sock");
    let broker = open_broker(&socket);
    let session = BusSession::connect(&socket, SessionOptions::named("svc")).unwrap();
    session.publish_retained(&Mode { night: false }).unwrap();
    session.publish_retained(&Mode { night: true }).unwrap();
    session
        .remember_raw("test.other", 1, vec![1, 2, 3])
        .unwrap();
    assert_eq!(session.retained_topics(), 2);
    drop(broker);
    // While the broker is away publishing fails, but the value is kept.
    let _ = session.publish_retained(&Mode { night: false });
    let broker = open_broker(&socket);
    assert!(session.client().wait_reconnected(1).unwrap() >= 2);
    let reader = Client::connect(&socket, ClientOptions::named("reader")).unwrap();
    let modes = reader.subscribe::<Mode>().unwrap();
    assert_eq!(modes.recv_timeout(WAIT).unwrap(), Mode { night: false });
    let others = reader.subscribe_pattern("test.other").unwrap();
    assert_eq!(others.recv_timeout(WAIT).unwrap().payload, vec![1, 2, 3]);
    assert!(session.republished() >= 2);
    assert_eq!(broker.stats().retained, 2);
    session.close();
    assert_eq!(session.client().wait_reconnected(u64::MAX - 1), None);
}

#[test]
fn registering_waits_for_a_name_that_is_being_released() {
    let dir = temp_dir("register");
    let socket = dir.join("bus.sock");
    let _broker = open_broker(&socket);
    let old = BusSession::connect(&socket, SessionOptions::named("old")).unwrap();
    let held = old.register("thing").unwrap();
    let mut options = SessionOptions::named("new");
    options.register_wait = Duration::ZERO;
    let impatient = BusSession::connect(&socket, options).unwrap();
    assert!(matches!(
        impatient.register("thing"),
        Err(BusError::Remote(ErrorCode::NameTaken))
    ));
    // The old instance goes away; the new one gets the name by retrying.
    let mut options = SessionOptions::named("new");
    options.register_wait = WAIT;
    options.backoff_min = Duration::from_millis(1);
    let patient = BusSession::connect(&socket, options).unwrap();
    drop(held);
    old.close();
    let endpoint = patient.register("thing").unwrap();
    drop(endpoint);
    // A refusal that is not temporary is reported at once.
    assert!(matches!(
        patient.register("Not A Name"),
        Err(BusError::InvalidName)
    ));
}

#[test]
fn the_number_of_remembered_topics_is_bounded() {
    let dir = temp_dir("bound");
    let socket = dir.join("bus.sock");
    let _broker = open_broker(&socket);
    let session = BusSession::connect(&socket, SessionOptions::named("t")).unwrap();
    for index in 0..MAX_RETAINED_TOPICS {
        session
            .remember_raw(&format!("bound.t{index}"), 1, vec![0])
            .unwrap();
    }
    assert!(matches!(
        session.remember_raw("bound.one_more", 1, vec![0]),
        Err(BusError::Remote(ErrorCode::LimitExceeded))
    ));
    // Replacing a known topic is always possible.
    session.remember_raw("bound.t0", 2, vec![9]).unwrap();
    assert!(matches!(
        session.remember_raw("Bad Topic", 1, vec![]),
        Err(BusError::InvalidName)
    ));
    assert_eq!(session.retained_topics(), MAX_RETAINED_TOPICS);
}

#[test]
fn a_token_from_a_secret_authenticates_the_session() {
    let dir = temp_dir("token");
    let socket = dir.join("bus.sock");
    let auth = ultar_bus::TokenIdentity::new().with_token("tok-0123456789", id("system.svc"));
    let _broker = Broker::builder(Policy::allow_all())
        .listen(&socket, Arc::new(auth))
        .start()
        .unwrap();
    let mut options = SessionOptions::named("svc");
    options.token = Some(Secret::from_bytes(b"tok-0123456789\n".to_vec()).unwrap());
    let session = BusSession::connect(&socket, options).unwrap();
    assert_eq!(session.client().identity(), "system.svc");
    let mut wrong = SessionOptions::named("svc");
    wrong.token = Some(Secret::from_bytes(b"tok-000".to_vec()).unwrap());
    wrong.connect_wait = WAIT;
    // A refused token is final: no retrying until the patience is over.
    assert!(matches!(
        BusSession::connect(&socket, wrong),
        Err(BusError::AuthFailed)
    ));
}
