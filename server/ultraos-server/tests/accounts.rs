//! Behaviours that existed before the public store: account, device number, TURN, VPN,
//! signalling and Verbund must keep working.

mod common;
use common::TestServer;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

const WG_KEY_1: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const WG_KEY_2: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";

#[test]
fn register_token_device_number_vpn_peer() {
    let s = TestServer::start();
    let reply = s.post(
        "/v1/account/register",
        None,
        &json!({"username":"anna","password":"password-123"}),
    );
    assert_eq!(reply.status, 200);
    let v = reply.json();
    assert_eq!(v["account_id"], 1);
    let token = v["token"].as_str().unwrap().to_string();

    let device = json!({"device_id":"device-1","pubkey":"cHVi"});
    let number = s.post("/v1/device/register", Some(&token), &device).json()["number"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(number.starts_with("UOS-") && number.len() == 13, "{number}");
    let again = s.post("/v1/device/register", Some(&token), &device).json();
    assert_eq!(again["number"], number.as_str(), "idempotent per device");
    let second = s.post(
        "/v1/device/register",
        Some(&token),
        &json!({"device_id":"device-2","pubkey":"cHVi"}),
    );
    assert_ne!(second.json()["number"], number.as_str());

    let peer = s
        .post("/v1/vpn/peer", Some(&token), &json!({"pubkey": WG_KEY_1}))
        .json();
    assert_eq!(peer["address"], "10.80.0.2/32");
    assert_eq!(peer["server_pubkey"], "server-pubkey");
    assert_eq!(peer["endpoint"], "ultar.code-craft.tech:51820");
    assert_eq!(peer["dns"], "10.80.0.1");
    assert_eq!(peer["allowed_ips"], "0.0.0.0/0, ::/0");
    let same = s
        .post("/v1/vpn/peer", Some(&token), &json!({"pubkey": WG_KEY_1}))
        .json();
    assert_eq!(
        same["address"], "10.80.0.2/32",
        "same key keeps its address"
    );
    let other = s
        .post("/v1/vpn/peer", Some(&token), &json!({"pubkey": WG_KEY_2}))
        .json();
    assert_eq!(other["address"], "10.80.0.3/32");
    let bad = s.post(
        "/v1/vpn/peer",
        Some(&token),
        &json!({"pubkey": "not-a-key"}),
    );
    assert_eq!(
        (bad.status, bad.error_code().as_str()),
        (400, "invalid_wireguard_key")
    );
}

#[test]
fn account_endpoints_reject_bad_input_and_missing_tokens() {
    let s = TestServer::start();
    let token = s.register("anna");
    let dup = s.post(
        "/v1/account/register",
        None,
        &json!({"username":"anna","password":"password-123"}),
    );
    assert_eq!(
        (dup.status, dup.error_code().as_str()),
        (409, "username_taken")
    );
    for body in [
        json!({"username":"ab","password":"password-123"}),
        json!({"username":"carl","password":"short"}),
    ] {
        let r = s.post("/v1/account/register", None, &body);
        assert_eq!(
            (r.status, r.error_code().as_str()),
            (400, "invalid_request")
        );
    }
    let ok = s.post(
        "/v1/account/login",
        None,
        &json!({"username":"anna","password":"password-123"}),
    );
    assert_eq!(ok.status, 200);
    assert!(ok.json()["token"].as_str().is_some());
    for body in [
        json!({"username":"anna","password":"wrong-password"}),
        json!({"username":"ghost","password":"password-123"}),
    ] {
        let r = s.post("/v1/account/login", None, &body);
        assert_eq!(
            (r.status, r.error_code().as_str()),
            (401, "invalid_credentials")
        );
    }
    // Account-bound endpoints stay closed without a token.
    let device = json!({"device_id":"d","pubkey":"p"});
    for (path, body) in [
        ("/v1/device/register", device.clone()),
        ("/v1/vpn/peer", json!({"pubkey": WG_KEY_1})),
        ("/v1/verbund/geraet", json!({"pk": "ab".repeat(32)})),
    ] {
        let r = s.post(path, None, &body);
        assert_eq!(r.status, 401, "{path}");
    }
    assert_eq!(s.get("/v1/turn-cred").error_code(), "auth_required");
    assert_eq!(s.get("/v1/verbund/geraete").status, 401);
    // A device id belongs to the account that registered it first.
    s.post("/v1/device/register", Some(&token), &device);
    let thief = s.register("mallory");
    let r = s.post("/v1/device/register", Some(&thief), &device);
    assert_eq!((r.status, r.error_code().as_str()), (409, "device_taken"));
}

#[test]
fn turn_credentials_follow_the_coturn_scheme() {
    use base64::Engine;
    use hmac::Mac;
    let s = TestServer::start();
    let token = s.register("anna");
    let v = s.get_as("/v1/turn-cred", &token).json();
    assert_eq!(v["ttl"], 3600);
    assert_eq!(v["urls"].as_array().unwrap().len(), 3);
    let user = v["username"].as_str().unwrap();
    assert!(user.ends_with(":1"));
    let mut mac = <hmac::Hmac<sha1::Sha1> as Mac>::new_from_slice(b"turn-secret").unwrap();
    mac.update(user.as_bytes());
    let expected = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    assert_eq!(v["credential"], expected.as_str());

    let unconfigured = TestServer::start_with(|c| c.turn_secret.clear());
    let token = unconfigured.register("anna");
    let r = unconfigured.get_as("/v1/turn-cred", &token);
    assert_eq!(
        (r.status, r.error_code().as_str()),
        (503, "turn_not_configured")
    );
}

#[test]
fn verbund_device_list() {
    let s = TestServer::start();
    let anna = s.register("anna");
    let pk = "ab".repeat(32);
    let r = s.post(
        "/v1/verbund/geraet",
        Some(&anna),
        &json!({"pk": pk, "name": "laptop", "ausgabe": "laptop"}),
    );
    assert_eq!(r.json()["ok"], true);
    let list = s.get_as("/v1/verbund/geraete", &anna).json();
    assert_eq!(list["geraete"][0]["name"], "laptop");
    assert_eq!(list["geraete"][0]["online"], false);
    let bert = s.register("bert");
    let foreign = s.post("/v1/verbund/geraet", Some(&bert), &json!({"pk": pk}));
    assert_eq!(foreign.status, 409, "a key belongs to one account");
    let invalid = s.post("/v1/verbund/geraet", Some(&anna), &json!({"pk": "xyz"}));
    assert_eq!(invalid.status, 400);
    let removed = s
        .post(
            "/v1/verbund/geraet/entfernen",
            Some(&anna),
            &json!({"pk": pk}),
        )
        .json();
    assert_eq!(removed["entfernt"], 1);
    assert_eq!(s.get("/v1/verbund/statistik").json()["verbindungen"], 0);
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(s: &TestServer, token: &str, number: &str) -> (Socket, Value) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/v1/signal", s.addr))
        .await
        .unwrap();
    ws.send(Message::Text(
        json!({"token": token, "number": number}).to_string(),
    ))
    .await
    .unwrap();
    let first = next_json(&mut ws).await;
    (ws, first)
}

async fn next_json(ws: &mut Socket) -> Value {
    loop {
        match ws.next().await.unwrap().unwrap() {
            Message::Text(t) => return serde_json::from_str(&t).unwrap(),
            Message::Close(_) => return json!({"closed": true}),
            _ => {}
        }
    }
}

fn number_for(s: &TestServer, token: &str, device: &str) -> String {
    let r = s.post(
        "/v1/device/register",
        Some(token),
        &json!({"device_id": device, "pubkey": "p"}),
    );
    r.json()["number"].as_str().unwrap().to_string()
}

#[test]
fn signalling_relays_between_numbers_and_sets_the_sender() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let (num_a, num_b) = (
        number_for(&s, &anna, "dev-a"),
        number_for(&s, &bert, "dev-b"),
    );
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let (mut a, ready_a) = connect(&s, &anna, &num_a).await;
        let (mut b, ready_b) = connect(&s, &bert, &num_b).await;
        assert_eq!(
            (ready_a["type"].as_str(), ready_b["type"].as_str()),
            (Some("ready"), Some("ready"))
        );

        let offer =
            json!({"to": num_b, "type": "offer", "data": {"sdp": "x"}, "from": "UOS-FAKE-FAKE"});
        a.send(Message::Text(offer.to_string())).await.unwrap();
        let got = next_json(&mut b).await;
        assert_eq!(got["type"], "offer");
        assert_eq!(
            got["from"],
            num_a.as_str(),
            "sender is set by the server, not the client"
        );
        assert_eq!(got["data"]["sdp"], "x");

        a.send(Message::Text(
            json!({"to": "UOS-0000-0000", "type": "offer"}).to_string(),
        ))
        .await
        .unwrap();
        let note = next_json(&mut a).await;
        assert_eq!(
            (note["type"].as_str(), note["to"].as_str()),
            (Some("unreachable"), Some("UOS-0000-0000"))
        );

        // A number of another account, or a bad token, is refused.
        let (_, refused) = connect(&s, &anna, &num_b).await;
        assert_eq!(refused["error"], "auth");
        let (_, refused) = connect(&s, "bad-token", &num_a).await;
        assert_eq!(refused["error"], "auth");
    });
}

async fn relay_connect(s: &TestServer, token: &str, pk: &str, role: &str) -> Socket {
    let url = format!("ws://{}/v1/verbund/relais", s.addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let hello = json!({"token": token, "pk": pk, "name": role, "ausgabe": "laptop", "rolle": role});
    ws.send(Message::Text(hello.to_string())).await.unwrap();
    assert_eq!(next_json(&mut ws).await["typ"], "bereit");
    ws
}

async fn next_binary(ws: &mut Socket) -> Vec<u8> {
    loop {
        if let Message::Binary(b) = ws.next().await.unwrap().unwrap() {
            return b;
        }
    }
}

fn frame(kind: u8, pk_hex: &str, session: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![kind];
    f.extend_from_slice(&hex::decode(pk_hex).unwrap());
    f.extend_from_slice(&[session; 8]);
    f.extend_from_slice(payload);
    f
}

#[test]
fn verbund_relay_forwards_only_within_one_account() {
    let s = TestServer::start();
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    let (service_pk, client_pk, foreign_pk) = ("aa".repeat(32), "bb".repeat(32), "cc".repeat(32));
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut service = relay_connect(&s, &anna, &service_pk, "dienst").await;
        let mut client = relay_connect(&s, &anna, &client_pk, "client").await;
        let mut foreign = relay_connect(&s, &bert, &foreign_pk, "dienst").await;

        // open: the service sees the SENDER key in place of the target key.
        client
            .send(Message::Binary(frame(1, &service_pk, 7, b"hello")))
            .await
            .unwrap();
        assert_eq!(
            next_binary(&mut service).await,
            frame(1, &client_pk, 7, b"hello")
        );
        // The reply travels back along the opened session.
        service
            .send(Message::Binary(frame(2, &client_pk, 7, b"world")))
            .await
            .unwrap();
        assert_eq!(
            next_binary(&mut client).await,
            frame(2, &service_pk, 7, b"world")
        );

        // A device of another account is unreachable, and it receives nothing.
        client
            .send(Message::Binary(frame(1, &foreign_pk, 9, b"secret")))
            .await
            .unwrap();
        assert_eq!(
            next_binary(&mut client).await,
            frame(4, &foreign_pk, 9, b"")
        );
        foreign
            .send(Message::Binary(frame(1, &service_pk, 3, b"intrude")))
            .await
            .unwrap();
        assert_eq!(
            next_binary(&mut foreign).await,
            frame(4, &service_pk, 3, b"")
        );

        let stats = s.get("/v1/verbund/statistik").json();
        assert_eq!(stats["verbindungen"], 3);
    });
}

#[test]
fn verbund_relay_refuses_a_bad_token() {
    let s = TestServer::start();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let url = format!("ws://{}/v1/verbund/relais", s.addr);
        let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let hello = json!({"token": "bad", "pk": "aa".repeat(32)});
        ws.send(Message::Text(hello.to_string())).await.unwrap();
        assert_eq!(next_json(&mut ws).await["typ"], "fehler");
    });
}
