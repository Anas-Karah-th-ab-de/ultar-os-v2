//! Security review r3 (device identity), finding R3-1: the server tells every connected device
//! when a lost-device recovery starts (`docs/DEVICE-IDENTITY.md` §5.7/§5.8, threat A3); the call
//! backend must hand that notice to the user interface.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tungstenite::Message;
use ultar_call::e2e::engine::Signer;
use ultar_call::e2e::media::{AudioIo, VideoIo};
use ultar_call::e2e::net::TurnConfig;
use ultar_call::e2e::pins::Pins;
use ultar_call::{CallBackend, CallEvent, Device, E2eCall, SignalAuth};

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

/// A device without key, relay, sound or camera.
struct Bare;

struct NoKey;
impl Signer for NoKey {
    fn public(&self) -> Option<[u8; 32]> {
        None
    }
    fn sign(&self, _m: &[u8]) -> Option<[u8; 64]> {
        None
    }
}

struct Silence;
impl AudioIo for Silence {
    fn capture(&mut self) -> Option<Vec<i16>> {
        None
    }
    fn play(&mut self, _frame: &[i16]) {}
}

impl Device for Bare {
    fn signer(&self) -> Box<dyn Signer> {
        Box::new(NoKey)
    }
    fn turn(&self) -> Option<TurnConfig> {
        None
    }
    fn audio(&self) -> Box<dyn AudioIo> {
        Box::new(Silence)
    }
    fn video(&self) -> Option<Box<dyn VideoIo>> {
        None
    }
    fn pins(&self) -> Pins {
        Pins::default()
    }
}

/// A signalling server that accepts one device and then pushes one security notice.
fn fake_server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let _hello = ws.read();
        let challenge = r#"{"type":"challenge","challenge":"AA","server":"x","account_id":1}"#;
        ws.send(Message::text(challenge)).unwrap();
        let _auth = ws.read();
        let ready = r#"{"type":"ready","number":"UOS-0001-0001"}"#;
        ws.send(Message::text(ready)).unwrap();
        let notice = r#"{"type":"notice","event":"recovery_started","device":"abcdefgh"}"#;
        ws.send(Message::Text(notice.into())).unwrap();
        std::thread::sleep(Duration::from_secs(5));
    });
    base
}

#[test]
fn r3_1_a_security_notice_from_the_server_reaches_the_user() {
    let mut backend = E2eCall::new(Arc::new(Bare));
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
