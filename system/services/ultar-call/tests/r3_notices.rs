//! Security review r3 (device identity), finding R3-1: the server tells every connected device
//! when a lost-device recovery starts (`docs/DEVICE-IDENTITY.md` §5.7/§5.8, threat A3) – and the
//! device throws the notice away (`real.rs`: `"ready" | "notice" => {}`). Nothing on the device
//! reads `/v1/device/events` either, so the owner is never told that his number is about to move.
//!
//! This test FAILS on the reviewed code. It needs the feature `real`
//! (`cargo test -p ultar-call --features real --test r3_notices`).

use futures_util::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;
use ultar_call::{CallEvent, SignalAuth};

/// Stands in for the device key: the fake server below does not check the signature.
struct AnyProof;

impl SignalAuth for AnyProof {
    fn hello(&self) -> Result<String, String> {
        Ok(r#"{"type":"hello","device_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.into())
    }
    fn answer(&self, _challenge_frame: &str) -> Result<String, String> {
        Ok(r#"{"type":"auth","signature":"AA"}"#.into())
    }
}

/// A signalling server that accepts one device and then pushes one security notice.
async fn serve_one(listener: tokio::net::TcpListener) {
    let (stream, _) = listener.accept().await.unwrap();
    let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
    let _hello = ws.next().await;
    let challenge = r#"{"type":"challenge","challenge":"AA","server":"x","account_id":1}"#;
    ws.send(Message::Text(challenge.into())).await.unwrap();
    let _auth = ws.next().await;
    let ready = r#"{"type":"ready","number":"UOS-0001-0001"}"#;
    ws.send(Message::Text(ready.into())).await.unwrap();
    let notice = r#"{"type":"notice","event":"recovery_started","device":"abcdefgh"}"#;
    ws.send(Message::Text(notice.into())).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
}

fn fake_server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            serve_one(listener).await;
        });
    });
    base
}

#[test]
fn r3_1_a_security_notice_from_the_server_reaches_the_user() {
    let mut backend = ultar_call::default_backend();
    backend.register(&fake_server(), Box::new(AnyProof));
    let begun = Instant::now();
    let mut events: Vec<CallEvent> = Vec::new();
    let mut signed_in = false;
    while begun.elapsed() < Duration::from_secs(4) {
        signed_in |= backend.ready();
        events.extend(backend.poll());
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(signed_in, "the device signed in: {events:?}");
    let shown = format!("{events:?}");
    assert!(
        shown.contains("recovery_started"),
        "the server announced a recovery, the device reported: {shown}"
    );
}

/// Added with the fix: the mock backend (default build) knows the notice event too, so the user
/// interface handles it in every build.
#[test]
fn the_notice_event_names_what_happened_and_to_which_device() {
    let notice = CallEvent::Notice {
        event: "recovery_started".into(),
        device: "abcdefgh".into(),
    };
    let shown = format!("{notice:?}");
    assert!(shown.contains("recovery_started") && shown.contains("abcdefgh"));
}
