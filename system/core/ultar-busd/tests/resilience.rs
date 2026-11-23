//! The real `ultar-busd` under stress: a consumer that never reads, and a
//! broker that is killed and restarted while a service holds retained
//! topics and a registration.

mod common;

use common::{busd, connect, send_signal, temp_dir, terminate, text_field, LogSink, WAIT};
use serde::{Deserialize, Serialize};
use std::os::unix::net::UnixStream;
use std::path::Path;
use ultar_bus::wire::{
    read_envelope, read_preamble, write_envelope, write_preamble, Envelope, Hello, Kind,
    PROTOCOL_VERSION,
};
use ultar_bus::{encode, BusError, ErrorCode, Message, Topic};
use ultar_service::{BusSession, SessionOptions};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
struct Status {
    value: u32,
}

impl Message for Status {
    const VERSION: u16 = 1;
}

impl Topic for Status {
    const TOPIC: &'static str = "test.status";
    const RETAINED: bool = true;
}

/// A raw connection that subscribes and then never reads.
fn deaf_subscriber(socket: &Path, pattern: &str) -> UnixStream {
    let mut stream = UnixStream::connect(socket).unwrap();
    write_preamble(&mut stream, PROTOCOL_VERSION).unwrap();
    assert_eq!(read_preamble(&mut stream).unwrap(), PROTOCOL_VERSION);
    let hello = Hello {
        client_name: "deaf".into(),
        token: None,
    };
    let hello =
        Envelope::new(Kind::Hello, 0).with_payload(PROTOCOL_VERSION, encode(&hello).unwrap());
    write_envelope(&mut stream, &hello, u32::MAX).unwrap();
    assert_eq!(
        read_envelope(&mut stream, 1 << 16).unwrap().kind,
        Kind::Welcome
    );
    let subscribe = Envelope::new(Kind::Subscribe, 1).with_target(pattern);
    write_envelope(&mut stream, &subscribe, u32::MAX).unwrap();
    assert_eq!(
        read_envelope(&mut stream, 1 << 16).unwrap().kind,
        Kind::Response
    );
    stream
}

#[test]
fn a_slow_consumer_is_dropped_and_the_drop_is_logged() {
    let dir = temp_dir("slow");
    let socket = dir.join("bus.sock");
    let sink = LogSink::start(&dir.join("log.sock"));
    let child = busd()
        .arg(&socket)
        .arg("--allow-all")
        .arg("--log-socket")
        .arg(&sink.path)
        .spawn()
        .unwrap();
    let publisher = connect(&socket, "publisher");
    let deaf = deaf_subscriber(&socket, "flood.*");
    // 16 MiB against a per-connection bound of 8 MiB and a reader that
    // never reads: the broker must drop it instead of growing or blocking.
    let payload = vec![0u8; 64 * 1024];
    for _ in 0..256 {
        publisher
            .publish_raw("flood.data", 1, false, payload.clone(), true)
            .unwrap();
    }
    let record = sink.expect("bus.slow_consumer_dropped");
    assert_eq!(text_field(&record, "peer"), Some("system"));
    // The publisher was never blocked or dropped.
    assert!(publisher.is_connected());
    drop(deaf);
    assert_eq!(terminate(child).0, Some(0));
}

#[test]
fn retained_topics_are_back_after_the_broker_was_killed_and_restarted() {
    let dir = temp_dir("retained");
    let socket = dir.join("bus.sock");
    let start = || busd().arg(&socket).arg("--allow-all").spawn().unwrap();
    let first = start();
    let session = BusSession::connect(&socket, SessionOptions::named("publisher")).unwrap();
    session.publish_retained(&Status { value: 41 }).unwrap();
    session.publish_retained(&Status { value: 42 }).unwrap();
    let endpoint = session.register("test").unwrap();
    assert_eq!(session.retained_topics(), 1);
    // (The client must outlive its subscription: dropping the last handle
    // closes the connection.)
    let early = connect(&socket, "early");
    let seen = early.subscribe::<Status>().unwrap();
    assert_eq!(seen.recv_timeout(WAIT).unwrap(), Status { value: 42 });

    // The broker dies without warning and comes back with empty memory.
    send_signal(first.id(), "KILL");
    first.wait_with_output().unwrap();
    let second = start();
    let generation = session.client().wait_reconnected(1).unwrap();
    assert!(generation >= 2);
    // A client that only starts now still learns the retained value …
    let late_client = connect(&socket, "late");
    let late = late_client.subscribe::<Status>().unwrap();
    assert_eq!(late.recv_timeout(WAIT).unwrap(), Status { value: 42 });
    // … and the service name is registered again: a call reaches it.
    let caller = connect(&socket, "caller");
    let answer = std::thread::spawn(move || {
        let request = endpoint.recv().unwrap();
        request.reply_raw(&7u32).unwrap();
    });
    let reply: u32 = loop {
        match caller.request("test", "ping", &0u32, WAIT) {
            Ok(reply) => break reply,
            // The re-registration is on its way.
            Err(BusError::Remote(ErrorCode::UnknownService)) => std::thread::yield_now(),
            Err(other) => panic!("{other}"),
        }
    };
    assert_eq!(reply, 7);
    answer.join().unwrap();
    assert!(session.republished() >= 1);
    assert_eq!(terminate(second).0, Some(0));
}
