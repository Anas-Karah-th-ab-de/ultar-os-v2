//! The real `ultar-llm-bus` between a test bus and a stand-in for the model service's
//! socket: the state follows the model service, a call is passed on, and without the model
//! service there is simply no model.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::Duration;
use ultar_bus::messages::llm::{Complete, LlmState, ModelClass};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.llm"
register = ["llm"]
publish = ["llm.state"]
call = ["settings.get", "init.switch"]
subscribe = ["settings.changed"]

[[rule]]
identity = "system.settingsd"
register = ["settings"]
publish = ["settings.changed"]

[[rule]]
identity = "system.init"
register = ["init"]

[[rule]]
identity = "system.agentd"
call = ["llm.complete"]
subscribe = ["llm.state"]
"#;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(20));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

fn reply(path: &str, request: &str) -> String {
    let body = match path {
        "/health" => r#"{"status":"ok","engine":"idle","model":null}"#.to_owned(),
        "/v1/models" => r#"{"object":"list","data":[
            {"id":"tiny","meta":{"bytes":400000000}},
            {"id":"qwen-4b","meta":{"bytes":2500000000}}]}"#
            .to_owned(),
        _ => {
            // Answers with the user's words in capitals, so the test sees they arrived.
            let json: serde_json::Value =
                serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
            let said = json["messages"][1]["content"]
                .as_str()
                .unwrap()
                .to_uppercase();
            serde_json::json!({"choices":[{"message":{"content": said}}]}).to_string()
        }
    };
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

/// Reads head and body: the bridge writes them in two steps.
fn whole_request(stream: &mut impl Read) -> String {
    let mut got = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let text = String::from_utf8_lossy(&got).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length = head
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length: "))
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= length {
                return text;
            }
        }
        let n = stream.read(&mut buffer).unwrap();
        if n == 0 {
            return String::from_utf8_lossy(&got).into_owned();
        }
        got.extend_from_slice(&buffer[..n]);
    }
}

/// The model service's socket: answers the three requests the bridge makes.
fn model_service(socket: &Path) {
    let listener = UnixListener::bind(socket).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let request = whole_request(&mut stream);
            let path = request.split_whitespace().nth(1).unwrap_or("");
            stream.write_all(reply(path, &request).as_bytes()).unwrap();
        }
    });
}

fn state_of(client: &Client, ready: bool) -> LlmState {
    let states = client.subscribe::<LlmState>().unwrap();
    let mut found = None;
    assert!(wait_until(WAIT, || {
        while let Ok(Some(state)) = states.try_recv() {
            if state.ready == ready {
                found = Some(state);
            }
        }
        found.is_some()
    }));
    found.unwrap()
}

#[test]
fn the_model_of_the_device_is_on_the_bus() {
    let world = World::new("llmbus", POLICY, &["system.llm", "system.agentd"]);
    let socket = world.subdir("llm").join("llm.sock");
    model_service(&socket);
    let args = ["--socket", socket.to_str().unwrap()];
    let _daemon = world.spawn(env!("CARGO_BIN_EXE_ultar-llm-bus"), "system.llm", &args);
    let helper = world.client("system.agentd");
    // No model is loaded: the largest installed one counts.
    let state = state_of(&helper, true);
    assert_eq!(
        (state.model.as_str(), state.class),
        ("qwen-4b", ModelClass::Medium)
    );
    let ask = Complete {
        system: "rules".into(),
        prompt: "hello".into(),
        max_tokens: 16,
    };
    let answer = call(&helper, &ask).unwrap();
    assert!(answer.ok);
    assert_eq!(answer.text, "HELLO");
}

#[test]
fn without_the_model_service_there_is_no_model() {
    let world = World::new("llmbus-none", POLICY, &["system.llm", "system.agentd"]);
    let socket = world.subdir("llm").join("llm.sock");
    let args = ["--socket", socket.to_str().unwrap()];
    let _daemon = world.spawn(env!("CARGO_BIN_EXE_ultar-llm-bus"), "system.llm", &args);
    let helper = world.client("system.agentd");
    assert_eq!(state_of(&helper, false), LlmState::default());
    let ask = Complete {
        system: "s".into(),
        prompt: "p".into(),
        max_tokens: 8,
    };
    assert!(!call(&helper, &ask).unwrap().ok);
}

#[test]
fn the_model_service_runs_while_the_person_has_the_assistant_switched_on() {
    use std::sync::{Arc, Mutex};
    use ultar_bus::messages::init::{ControlReply, ServiceState, Switch};
    use ultar_bus::messages::settings::{Changed, Get, GetReply, SettingValue};
    let ids = [
        "system.llm",
        "system.agentd",
        "system.settingsd",
        "system.init",
    ];
    let world = World::new("llmbus-switch", POLICY, &ids);
    world.serve("system.settingsd", "settings", |request| {
        let asked = request.decode::<Get>().map(|g| g.key).unwrap_or_default();
        assert_eq!(asked, "assistant.enabled");
        let value = Some(SettingValue::Flag(true));
        let _ = request.reply(&GetReply { value });
    });
    let switched = Arc::new(Mutex::new(Vec::new()));
    let seen = switched.clone();
    world.serve("system.init", "init", move |request| {
        if let Ok(switch) = request.decode::<Switch>() {
            seen.lock().unwrap().push((switch.service, switch.on));
        }
        let state = ServiceState::Running;
        let _ = request.reply(&ControlReply { state });
    });
    let socket = world.subdir("llm").join("llm.sock");
    let args = ["--socket", socket.to_str().unwrap()];
    let _daemon = world.spawn(env!("CARGO_BIN_EXE_ultar-llm-bus"), "system.llm", &args);
    let is = |wanted: &[(&str, bool)]| {
        let got = switched.lock().unwrap().clone();
        got.iter()
            .map(|(s, on)| (s.as_str(), *on))
            .eq(wanted.iter().copied())
    };
    // at start it reads the setting: on
    assert!(wait_until(WAIT, || is(&[("llm", true)])));
    // the person switches the assistant off, then on again; another key is not its business
    let settings = world.client("system.settingsd");
    let say = |key: &str, on: bool| {
        let changed = Changed {
            key: key.into(),
            value: SettingValue::Flag(on),
        };
        settings.publish(&changed).unwrap();
    };
    say("assistant.wake_word", false);
    say("assistant.enabled", false);
    assert!(wait_until(WAIT, || is(&[("llm", true), ("llm", false)])));
    say("assistant.enabled", true);
    assert!(wait_until(WAIT, || is(&[
        ("llm", true),
        ("llm", false),
        ("llm", true)
    ])));
}
