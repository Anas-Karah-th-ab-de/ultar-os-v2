//! Live check: the device client against the running release server (not part of the normal run).
//!
//! ```text
//! ULTAR_LIVE_USER=<account> ULTAR_LIVE_PASSWORD=<password> ULTAR_LIVE_HOME=<dir that persists> \
//!   cargo test --release --test live_server -- --ignored --nocapture
//! ```
//! `ULTAR_LIVE_BASE` defaults to `https://ultar.code-craft.tech`. The account is created on the first
//! run; the device key and number stay in `ULTAR_LIVE_HOME`, so later runs reuse the same device
//! (the server limits how many devices an account may ever register).
//!
//! Covers what a device does with the server: store and updates without account, sign-in, device
//! registration bound to the server's name, a device session, the signalling socket (calls), TURN
//! credentials and the Verbund relay (two connections of one account see each other).

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use ultar_cloud::{Client, CloudError, DeviceHome};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

fn base() -> String {
    std::env::var("ULTAR_LIVE_BASE").unwrap_or_else(|_| "https://ultar.code-craft.tech".into())
}

fn ws_base() -> String {
    base()
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1)
}

/// The account client and its token; the account is created on the first run.
fn signed_in() -> (Client, String) {
    let (user, password) = (env("ULTAR_LIVE_USER"), env("ULTAR_LIVE_PASSWORD"));
    let anonymous = Client::new(&base(), None);
    let token = match anonymous.login(&user, &password) {
        Ok(token) => token,
        Err(CloudError::InvalidCredentials) => {
            anonymous.register(&user, &password).expect("register")
        }
        Err(other) => panic!("login: {other:?}"),
    };
    (Client::new(&base(), Some(token.clone())), token)
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The next text frame, as sent.
async fn raw(ws: &mut Socket) -> String {
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(15), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => return t,
            Ok(Some(Ok(_))) => continue,
            other => panic!("no text frame: {other:?}"),
        }
    }
}

async fn text(ws: &mut Socket) -> Value {
    serde_json::from_str(&raw(ws).await).unwrap()
}

/// The signalling socket (calls): the device proves itself for exactly this server.
async fn signal(home: &DeviceHome, number: &str) {
    let proof = home.signal_proof(&base()).expect("signal proof");
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{}/v1/signal", ws_base()))
        .await
        .expect("signal socket");
    ws.send(Message::Text(proof.hello().unwrap()))
        .await
        .unwrap();
    let challenge = raw(&mut ws).await;
    ws.send(Message::Text(proof.answer(&challenge).expect("answer")))
        .await
        .unwrap();
    let ready = text(&mut ws).await;
    assert_eq!(
        (ready["type"].as_str(), ready["number"].as_str()),
        (Some("ready"), Some(number)),
        "{ready}"
    );
    println!("ok signalling socket: ready as {number}");
}

/// The Verbund relay as the mesh service uses it: two devices of the account sign in (role
/// "dienst"), see each other in the list and a frame reaches the other one with the sender's
/// key in place of the target's.
async fn relay(token: &str) {
    let keys = [[0x11_u8; 32], [0x22_u8; 32]];
    let hex = |k: &[u8; 32]| k.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let mut socks = Vec::new();
    for (key, name) in keys.iter().zip(["live-a", "live-b"]) {
        let (mut ws, _) =
            tokio_tungstenite::connect_async(format!("{}/v1/verbund/relais", ws_base()))
                .await
                .expect("relay socket");
        let hello = json!({"token":token,"pk":hex(key),"name":name,"ausgabe":"desktop",
            "rolle":"dienst","kerne":0,"frei":0});
        ws.send(Message::Text(hello.to_string())).await.unwrap();
        assert_eq!(text(&mut ws).await["typ"], "bereit");
        socks.push(ws);
    }
    let mut seen = 0;
    while seen < 2 {
        let m = text(&mut socks[0]).await;
        if m["typ"] == "geraete" {
            seen = m["liste"].as_array().map_or(0, Vec::len);
        }
    }
    // open (type 1) from a to b, session 8 bytes, payload "hi"
    let mut frame = vec![1u8];
    frame.extend_from_slice(&keys[1]);
    frame.extend_from_slice(&[7; 8]);
    frame.extend_from_slice(b"hi");
    socks[0].send(Message::Binary(frame)).await.unwrap();
    let got = loop {
        match tokio::time::timeout(std::time::Duration::from_secs(15), socks[1].next()).await {
            Ok(Some(Ok(Message::Binary(b)))) => break b,
            Ok(Some(Ok(_))) => continue,
            other => panic!("no frame at b: {other:?}"),
        }
    };
    assert_eq!(
        (got[0], &got[1..33], &got[41..]),
        (1, &keys[0][..], &b"hi"[..])
    );
    println!("ok verbund relay: both devices listed, a frame went from a to b");
}

/// TURN credentials for calls.
fn turn(token: &str) {
    let turn: Value = ureq::get(&format!("{}/v1/turn-cred", base()))
        .set("Authorization", &format!("Bearer {}", token))
        .call()
        .expect("turn-cred")
        .into_json()
        .unwrap();
    let urls: Vec<&str> = turn["urls"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        urls.iter().any(|u| u.starts_with("turn:")) && turn["credential"].is_string(),
        "{turn}"
    );
    println!("ok turn credentials: {urls:?}");
}

#[test]
#[ignore = "needs the network and an account for the live server"]
fn a_device_works_with_the_live_server() {
    // 1. Without account: store, categories, updates.
    let anonymous = Client::new(&base(), None);
    anonymous.categories().expect("categories");
    anonymous.store_list().expect("store list");
    println!("ok store and categories without account");

    // 2. Account and device: the device signs for the server's own name.
    let (account, token) = signed_in();
    assert_eq!(
        account.server_name().unwrap(),
        ultar_cloud::identity::server_name(&base()).unwrap()
    );
    let home = DeviceHome::new(env("ULTAR_LIVE_HOME"));
    let number = home
        .register_number(&account, "live-check", None)
        .expect("device registration");
    assert!(number.starts_with("UOS-"), "{number}");
    let device = account
        .as_device(&home.key_store().unwrap())
        .expect("device session");
    device.entitlements().expect("entitlements as device");
    assert!(device.devices().unwrap().iter().any(|d| d.current));
    println!(
        "ok account, device {} number {number}",
        home.device_id().unwrap()
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        signal(&home, &number).await;
        relay(&token).await;
    });
    turn(&token);
}
