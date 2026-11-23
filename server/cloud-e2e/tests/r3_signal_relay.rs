//! Security review r3 (device identity), finding R3-3: the signalling sign-in signs for the
//! server name in the device's *stored* registration, not for the server it is talking to.
//!
//! `docs/DEVICE-IDENTITY.md` §4, threat A6: "the device derives [the server name] from its own
//! configured server address and refuses to sign when the name in a challenge differs", so that a
//! malicious server cannot obtain a signature that is usable on the real one. The HTTP calls do
//! that (`Client::device_challenge`). The call sign-in does not: `ultar-shell`
//! (`app_phone.rs`, `CloudTelephony::default`) connects to `ultar_cloud::base_url()` and answers
//! with `signal_auth(key, identity.server, identity.account_id, frame)`. A device that is pointed
//! at another server after it registered (settings, a "test server", a tampered `cloud.conf`)
//! signs the real server's challenge for that server, which then holds the device's socket –
//! and its incoming calls – on the real server.
//!
//! This test FAILS on the reviewed code.

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use ultar_cloud::DeviceHome;
use ultar_cloud_e2e::Stack;

type Ws<S> = tokio_tungstenite::WebSocketStream<S>;

async fn text<S>(ws: &mut Ws<S>) -> Option<String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    match ws.next().await {
        Some(Ok(Message::Text(t))) => Some(t),
        _ => None,
    }
}

/// The other server: passes the device's frames to the real server and keeps the socket it gets.
/// Returns the real server's last answer, or `{"refused":true}` when the device signed nothing.
async fn relay(listener: tokio::net::TcpListener, real: String) -> Value {
    let (stream, _) = listener.accept().await.unwrap();
    let mut device = tokio_tungstenite::accept_async(stream).await.unwrap();
    let (mut upstream, _) = tokio_tungstenite::connect_async(real).await.unwrap();
    let Some(hello) = text(&mut device).await else {
        return json!({"refused": true});
    };
    upstream.send(Message::Text(hello)).await.unwrap();
    let challenge = text(&mut upstream).await.unwrap();
    device.send(Message::Text(challenge)).await.unwrap();
    let Some(auth) = text(&mut device).await else {
        return json!({"refused": true});
    };
    upstream.send(Message::Text(auth)).await.unwrap();
    let ready: Value = serde_json::from_str(&text(&mut upstream).await.unwrap()).unwrap();
    // Held open so that the caller can see the number online at the real server.
    std::mem::forget(upstream);
    ready
}

/// The sign-in as the shell does it (`app_phone.rs`, `CloudTelephony::default` with `real.rs`):
/// socket to the configured address, proof from `DeviceHome::signal_proof` **for that address**.
/// (Body replaced with the fix of R3-3, as the reviewer's note asked; the assertion below is
/// unchanged.)
/// Returns false when the device did not even connect (it has no proof for that address).
async fn sign_in_like_the_shell(home: &DeviceHome, configured_base: &str) -> bool {
    let Ok(proof) = home.signal_proof(configured_base) else {
        return false;
    };
    let url = format!("{}/v1/signal", configured_base.replace("http://", "ws://"));
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    ws.send(Message::Text(proof.hello().unwrap()))
        .await
        .unwrap();
    let Some(challenge) = text(&mut ws).await else {
        return true;
    };
    if let Ok(auth) = proof.answer(&challenge) {
        let _ = ws.send(Message::Text(auth)).await;
        let _ = text(&mut ws).await;
    }
    true
}

#[test]
fn r3_3_a_device_pointed_at_another_server_does_not_sign_for_the_real_one() {
    let real = Stack::start();
    let anna = real.signed_in("anna");
    let home = real.device_home("phone");
    let number = home.register_number(&anna, "phone", None).unwrap();
    assert!(!real.state.hub.is_online(&number));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let answer = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let other_base = format!("http://{}", listener.local_addr().unwrap());
        let real_ws = format!("ws://{}/v1/signal", real.addr);
        let relayed = tokio::spawn(relay(listener, real_ws));
        // The device now has the other server's address in its configuration.
        if sign_in_like_the_shell(&home, &other_base).await {
            relayed.await.unwrap()
        } else {
            // The device never connected: the relay has nothing to pass on.
            relayed.abort();
            json!({"refused": true})
        }
    });

    assert_ne!(
        answer["type"], "ready",
        "the other server signed in at the real server as {number}: {answer}"
    );
    assert!(
        !real.state.hub.is_online(&number),
        "the other server holds the socket of {number} at the real server"
    );
}
