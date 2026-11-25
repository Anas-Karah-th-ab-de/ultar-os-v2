//! The real `ultar-llm-cloud` between a test bus and a stand-in for the provider: nothing
//! leaves the device without the person's key, the key reaches the provider and nobody
//! else, and a refused key is no answer.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::llm_cloud::{ClearKey, CloudComplete, CloudState, Saved, SetKey};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.llmcloud"
register = ["llmcloud"]
publish = ["llmcloud.state"]

[[rule]]
identity = "system.agentd"
call = ["llmcloud.complete"]
subscribe = ["llmcloud.state"]

[[rule]]
identity = "app.settings"
call = ["llmcloud.set_key", "llmcloud.clear_key"]
subscribe = ["llmcloud.state"]
"#;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(20));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

/// The provider: accepts the key `sk-good`, writes down every request it got.
fn provider(seen: &Arc<Mutex<Vec<String>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let seen = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut got = Vec::new();
            let mut buffer = [0u8; 4096];
            let request = loop {
                let text = String::from_utf8_lossy(&got).into_owned();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length = head.to_lowercase().lines().find_map(|l| {
                        l.strip_prefix("content-length: ")
                            .and_then(|n| n.parse::<usize>().ok())
                    });
                    if body.len() >= length.unwrap_or(0) {
                        break text;
                    }
                }
                let n = stream.read(&mut buffer).unwrap();
                got.extend_from_slice(&buffer[..n]);
            };
            let good = request.to_lowercase().contains("x-api-key: sk-good");
            seen.lock().unwrap().push(request);
            let (status, body) = if good {
                let text =
                    r#"{"type":"message","content":[{"type":"text","text":"{\"say\":\"hi\"}"}]}"#;
                ("200 OK", text)
            } else {
                let text = r#"{"type":"error","error":{"type":"authentication_error"}}"#;
                ("401 Unauthorized", text)
            };
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    address
}

fn configured(client: &Client, wanted: bool) -> CloudState {
    let states = client.subscribe::<CloudState>().unwrap();
    let mut found = None;
    assert!(wait_until(WAIT, || {
        while let Ok(Some(state)) = states.try_recv() {
            if state.configured == wanted {
                found = Some(state);
            }
        }
        found.is_some()
    }));
    found.unwrap()
}

#[test]
fn nothing_leaves_the_device_without_the_persons_key() {
    let ids = ["system.llmcloud", "system.agentd", "app.settings"];
    let world = World::new("llmcloud", POLICY, &ids);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let endpoint = provider(&seen);
    let state = world.subdir("state");
    let args = ["--state", state.to_str().unwrap(), "--endpoint", &endpoint];
    let _daemon = world.spawn(
        env!("CARGO_BIN_EXE_ultar-llm-cloud"),
        "system.llmcloud",
        &args,
    );
    let (helper, settings) = (world.client("system.agentd"), world.client("app.settings"));
    let ask = CloudComplete {
        system: "rules".into(),
        prompt: "hello".into(),
        max_tokens: 64,
    };
    let state = configured(&helper, false);
    assert_eq!(state.provider, "Claude");
    assert!(!call(&helper, &ask).unwrap().ok);
    assert!(seen.lock().unwrap().is_empty(), "no key: no request");

    // the helper cannot set a key; the Settings app can, and cannot think with it
    let set = |key: &str| SetKey { key: key.into() };
    assert!(call(&helper, &set("sk-good")).is_err());
    assert!(call(&settings, &ask).is_err());
    assert_eq!(
        call(&settings, &set("not a key")).unwrap(),
        Saved { ok: false }
    );
    assert_eq!(
        call(&settings, &set("sk-good")).unwrap(),
        Saved { ok: true }
    );
    configured(&helper, true);
    let answer = call(&helper, &ask).unwrap();
    assert!(answer.ok);
    assert_eq!(answer.text, "{\"say\":\"hi\"}");
    let request = seen.lock().unwrap()[0].clone();
    assert!(request.starts_with("POST /v1/messages "), "{request}");
    assert!(request.to_lowercase().contains("anthropic-version: "));
    let body: serde_json::Value =
        serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["system"], "rules");
    assert_eq!(body["messages"][0]["content"], "hello");
    assert_eq!(body["max_tokens"], 64);

    // a key the provider refuses is no answer; a cleared key sends nothing again
    assert_eq!(call(&settings, &set("sk-bad")).unwrap(), Saved { ok: true });
    assert!(!call(&helper, &ask).unwrap().ok);
    assert_eq!(call(&settings, &ClearKey {}).unwrap(), Saved { ok: true });
    configured(&helper, false);
    let before = seen.lock().unwrap().len();
    assert!(!call(&helper, &ask).unwrap().ok);
    assert_eq!(seen.lock().unwrap().len(), before);
}
