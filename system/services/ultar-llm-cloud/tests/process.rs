//! The real `ultar-llm-cloud` between a test bus and a stand-in for a provider: nothing
//! leaves the device without the person's key, a refused key is not stored, the model is
//! one the provider offers, and a key never follows to another server.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::llm_cloud::{
    ClearKey, CloudComplete, CloudState, Configure, Refusal, Saved, SetKey,
};
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
call = ["llmcloud.set_key", "llmcloud.clear_key", "llmcloud.configure"]
subscribe = ["llmcloud.state"]
"#;

type Seen = Arc<Mutex<Vec<String>>>;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(30));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

fn whole_request(stream: &mut impl Read) -> String {
    let mut got = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let text = String::from_utf8_lossy(&got).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let head = head.to_lowercase();
            let length = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "));
            if body.len() >= length.and_then(|n| n.parse().ok()).unwrap_or(0) {
                return text;
            }
        }
        match stream.read(&mut buffer) {
            Ok(n) if n > 0 => got.extend_from_slice(&buffer[..n]),
            _ => return String::from_utf8_lossy(&got).into_owned(),
        }
    }
}

fn answer(request: &str) -> (&'static str, &'static str) {
    if !request
        .to_lowercase()
        .contains("authorization: bearer sk-good")
    {
        return (
            "401 Unauthorized",
            r#"{"type":"error","error":{"type":"authentication_error"}}"#,
        );
    }
    let path = request.split_whitespace().nth(1).unwrap_or("");
    let body = if path.starts_with("/v1/models") {
        r#"{"data":[{"type":"model","id":"claude-opus-5-5","display_name":"Claude Opus 5.5"},
            {"type":"model","id":"claude-haiku-5-5","display_name":"Claude Haiku 5.5"}],"has_more":false}"#
    } else if path == "/v1/messages" {
        r#"{"type":"message","content":[{"type":"text","text":"from messages"}]}"#
    } else {
        r#"{"choices":[{"message":{"role":"assistant","content":"from chat"}}]}"#
    };
    ("200 OK", body)
}

/// A provider that accepts the key `sk-good` and writes down every request it got.
fn provider(seen: &Seen) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let seen = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let request = whole_request(&mut stream);
            let (status, body) = answer(&request);
            seen.lock().unwrap().push(request);
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    address
}

fn state(client: &Client, wanted: impl Fn(&CloudState) -> bool) -> CloudState {
    let states = client.subscribe::<CloudState>().unwrap();
    let mut found = None;
    assert!(wait_until(WAIT, || {
        while let Ok(Some(state)) = states.try_recv() {
            if wanted(&state) {
                found = Some(state);
            }
        }
        found.is_some()
    }));
    found.unwrap()
}

fn ask() -> CloudComplete {
    CloudComplete {
        system: "rules".into(),
        prompt: "hello".into(),
        max_tokens: 64,
    }
}

fn set(key: &str) -> SetKey {
    SetKey { key: key.into() }
}

fn refused(refusal: Refusal) -> Saved {
    Saved { ok: false, refusal }
}

struct Bench {
    _world: World,
    _daemon: ultar_svckit::testkit::Daemon,
    helper: Client,
    settings: Client,
    claude: Seen,
}

fn bench(tag: &str) -> Bench {
    let ids = ["system.llmcloud", "system.agentd", "app.settings"];
    let world = World::new(tag, POLICY, &ids);
    let claude = Seen::default();
    let endpoint = provider(&claude);
    let (dir, seal) = (world.subdir("state"), world.subdir("seal"));
    let args = [
        ["--state", dir.to_str().unwrap()],
        ["--seal", seal.to_str().unwrap()],
        ["--claude-endpoint", &endpoint],
    ];
    let program = env!("CARGO_BIN_EXE_ultar-llm-cloud");
    let daemon = world.spawn(program, "system.llmcloud", &args.concat());
    let (helper, settings) = (world.client("system.agentd"), world.client("app.settings"));
    Bench {
        _world: world,
        _daemon: daemon,
        helper,
        settings,
        claude,
    }
}

#[test]
fn nothing_leaves_the_device_without_a_key_the_provider_accepts() {
    let b = bench("llmcloud");
    let first = state(&b.helper, |s| !s.configured);
    let ids: Vec<&str> = first.providers.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        (first.provider.as_str(), ids),
        ("claude", vec!["claude", "compatible"])
    );
    assert!(!call(&b.helper, &ask()).unwrap().ok);
    assert!(b.claude.lock().unwrap().is_empty(), "no key: no request");

    // the helper cannot set a key; the Settings app can, and cannot think with it
    assert!(call(&b.helper, &set("sk-good")).is_err());
    assert!(call(&b.settings, &ask()).is_err());
    assert_eq!(
        call(&b.settings, &set("not a key")).unwrap(),
        refused(Refusal::Invalid)
    );
    // a key the provider refuses is not stored
    assert_eq!(
        call(&b.settings, &set("sk-bad")).unwrap(),
        refused(Refusal::KeyRefused)
    );
    assert!(!call(&b.helper, &ask()).unwrap().ok);
    assert_eq!(
        b.claude.lock().unwrap().len(),
        1,
        "only the check of the refused key"
    );

    assert_eq!(
        call(&b.settings, &set("sk-good")).unwrap(),
        Saved::default_ok()
    );
    let ready = state(&b.helper, |s| s.configured && s.verified);
    // the small model the provider offers, not a name built into this service
    assert_eq!(ready.model, "claude-haiku-5-5");
    assert_eq!(ready.models.len(), 2);
    let said = call(&b.helper, &ask()).unwrap();
    assert_eq!((said.ok, said.text.as_str()), (true, "from messages"));
    let request = b.claude.lock().unwrap().last().unwrap().clone();
    assert!(request.starts_with("POST /v1/messages "), "{request}");
    assert!(request
        .to_lowercase()
        .contains("anthropic-version: 2026-10-21"));
    let body = request.split("\r\n\r\n").nth(1).unwrap();
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["model"], "claude-haiku-5-5");
    assert_eq!(
        (&body["system"], &body["max_tokens"]),
        (&"rules".into(), &64.into())
    );
    assert_eq!(body["messages"][0]["content"], "hello");
    forgotten_key_sends_nothing(&b);
}

/// After "forget the key" no request leaves the device.
fn forgotten_key_sends_nothing(b: &Bench) {
    assert_eq!(
        call(&b.settings, &ClearKey {}).unwrap(),
        Saved::default_ok()
    );
    state(&b.helper, |s| !s.configured);
    let before = b.claude.lock().unwrap().len();
    assert!(!call(&b.helper, &ask()).unwrap().ok);
    assert_eq!(b.claude.lock().unwrap().len(), before);
}

fn choose(provider: &str, endpoint: &str, model: &str) -> Configure {
    Configure {
        provider: provider.into(),
        endpoint: endpoint.into(),
        model: model.into(),
    }
}

#[test]
fn the_person_chooses_model_and_provider_and_a_key_never_follows_to_another_server() {
    let b = bench("llmcloud-choice");
    assert_eq!(
        call(&b.settings, &set("sk-good")).unwrap(),
        Saved::default_ok()
    );
    state(&b.helper, |s| s.verified);
    // another model of the same provider: the key stays
    let big = choose("claude", "", "claude-opus-5-5");
    assert_eq!(call(&b.settings, &big).unwrap(), Saved::default_ok());
    let chosen = state(&b.helper, |s| s.model == "claude-opus-5-5");
    assert!(chosen.configured);
    assert!(call(&b.helper, &ask()).unwrap().ok);
    let request = b.claude.lock().unwrap().last().unwrap().clone();
    assert!(
        request.contains("\"model\":\"claude-opus-5-5\""),
        "{request}"
    );

    // what is no provider, or an address a key must not go to, is refused
    let invalid = refused(Refusal::Invalid);
    assert_eq!(
        call(&b.settings, &choose("nobody", "", "")).unwrap(),
        invalid
    );
    let plain = choose("compatible", "http://example.org", "");
    assert_eq!(call(&b.settings, &plain).unwrap(), invalid);
    assert!(state(&b.helper, |s| s.provider == "claude").configured);

    // a server of the person's own choice: the key entered for Claude does not go there
    let other = Seen::default();
    let address = provider(&other);
    let own = choose("compatible", &address, "");
    assert_eq!(call(&b.settings, &own).unwrap(), Saved::default_ok());
    let moved = state(&b.helper, |s| s.provider == "compatible");
    assert!(!moved.configured && moved.models.is_empty());
    assert_eq!(moved.endpoint, address);
    assert!(!call(&b.helper, &ask()).unwrap().ok);
    assert!(
        other.lock().unwrap().is_empty(),
        "nothing was sent to the new server"
    );

    assert_eq!(
        call(&b.settings, &set("sk-good")).unwrap(),
        Saved::default_ok()
    );
    state(&b.helper, |s| s.configured && s.verified);
    let said = call(&b.helper, &ask()).unwrap();
    assert_eq!((said.ok, said.text.as_str()), (true, "from chat"));
    let request = other.lock().unwrap().last().unwrap().clone();
    assert!(
        request.starts_with("POST /v1/chat/completions "),
        "{request}"
    );
}
