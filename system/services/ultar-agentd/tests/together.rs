//! Mesh and helper together, as on two devices with the rules that are shipped
//! (`rootfs/etc/ultar/bus-policy.toml`): a phone without a model and a computer with one.
//! Real `ultar-mesh` and `ultar-agentd` on both, real `ultar-llm-bus` on the computer in
//! front of a stand-in for the model service's socket. The phone's helper thinks with the
//! computer's model, acts on the phone, and asks the computer's helper to act there.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::agent::{AgentState, Answer, Ask, Brain, StepOutcome};
use ultar_bus::messages::audio::{AudioOutcome, AudioReply, SetMute};
use ultar_bus::messages::launcher::{AppStatus, InstanceState, List, ListReply, Start, StartReply};
use ultar_bus::messages::mesh::{
    Done, MeshState, PairBegin, PairJoin, Right, SetEnabled, SetRights,
};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

type Script = Arc<Mutex<VecDeque<String>>>;
type Seen = Arc<Mutex<Vec<String>>>;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The other two services, built next to this crate's own binary.
fn programs() -> (String, String) {
    let packages = ["-p", "ultar-mesh", "-p", "ultar-llm-bus"];
    let built = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--locked", "--bins"])
        .args(packages)
        .current_dir(repo())
        .status()
        .unwrap();
    assert!(built.success(), "ultar-mesh and ultar-llm-bus must build");
    let dir = Path::new(env!("CARGO_BIN_EXE_ultar-agentd"))
        .parent()
        .unwrap();
    let at = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    (at("ultar-mesh"), at("ultar-llm-bus"))
}

fn call<C: Call>(client: &Client, c: &C, wait: u64) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(wait));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

fn free_port() -> u16 {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    socket.local_addr().unwrap().port()
}

fn whole_request(stream: &mut impl Read) -> String {
    let mut got = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let text = String::from_utf8_lossy(&got).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length = head
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length: "));
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

/// The model service's socket: one medium model that says the next line of the script.
fn model_service(socket: &Path, script: &Script) {
    let listener = UnixListener::bind(socket).unwrap();
    let script = script.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let request = whole_request(&mut stream);
            let body = match request.split_whitespace().nth(1).unwrap_or("") {
                "/health" => r#"{"status":"ok","model":"qwen-4b"}"#.to_owned(),
                "/v1/models" => {
                    r#"{"data":[{"id":"qwen-4b","meta":{"bytes":2500000000}}]}"#.to_owned()
                }
                _ => {
                    let said = script.lock().unwrap().pop_front().unwrap_or_default();
                    serde_json::json!({"choices":[{"message":{"content": said}}]}).to_string()
                }
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
}

/// Launcher and sound of a device: what they were asked is written down.
fn stand_ins(world: &World, seen: &Seen) {
    let s = seen.clone();
    world.serve("system.launcher", "launcher", move |request| {
        if request.method() == List::METHOD {
            let app = |id: &str| AppStatus {
                app: id.to_owned(),
                uid: 0,
                profile: String::new(),
                version: String::new(),
                instances: Vec::new(),
            };
            let apps = vec![app("files"), app("settings")];
            let _ = request.reply(&ListReply { apps });
        } else if let Ok(start) = request.decode::<Start>() {
            s.lock().unwrap().push(format!("open {}", start.app));
            let _ = request.reply(&StartReply {
                state: InstanceState::Running,
                pid: 1,
                already: false,
                opened: true,
            });
        }
    });
    let s = seen.clone();
    world.serve("system.audiod", "audio", move |request| {
        if let Ok(mute) = request.decode::<SetMute>() {
            s.lock().unwrap().push(format!("mute {}", mute.on));
        }
        let outcome = AudioOutcome::Done;
        let _ = request.reply(&AudioReply { outcome });
    });
}

struct Device {
    world: World,
    _daemons: Vec<Daemon>,
    shell: Client,
    seen: Seen,
}

impl Device {
    fn mesh(&self) -> MeshState {
        let states = self.shell.subscribe::<MeshState>().unwrap();
        states.recv_timeout(WAIT).unwrap()
    }

    fn peer_called(&self, name: &str, paired: bool) -> String {
        let mut id = None;
        assert!(wait_until(WAIT, || {
            let state = self.mesh();
            let peer = state
                .peers
                .iter()
                .find(|p| p.name == name && p.paired == paired);
            id = peer.map(|p| p.id.clone());
            id.is_some()
        }));
        id.unwrap()
    }

    fn brain(&self, wanted: &Brain) {
        let states = self.shell.subscribe::<AgentState>().unwrap();
        let mut last = None;
        let found = wait_until(Duration::from_secs(40), || {
            while let Ok(Some(state)) = states.try_recv() {
                last = Some(state.brain);
            }
            last.as_ref() == Some(wanted)
        });
        assert!(found, "the helper thinks with {last:?}, not {wanted:?}");
    }

    fn ask(&self, text: &str) -> Answer {
        call(&self.shell, &Ask { text: text.into() }, 60).unwrap()
    }

    fn took(&self) -> Vec<String> {
        std::mem::take(&mut self.seen.lock().unwrap())
    }
}

fn device(tag: &str, name: &str, kind: &str, ports: (u16, u16), model: Option<&Script>) -> Device {
    let (mesh, llm_bus) = programs();
    let policy = std::fs::read_to_string(repo().join("rootfs/etc/ultar/bus-policy.toml")).unwrap();
    let identities = [
        "system.mesh",
        "system.agentd",
        "system.llmbus",
        "system.launcher",
        "system.sysdialog",
        "system.audiod",
        "system.shell",
    ];
    let world = World::new(tag, &policy, &identities);
    let seen = Seen::default();
    stand_ins(&world, &seen);
    let (state, inbox) = (world.subdir("state"), world.subdir("inbox"));
    let (port, peer) = (ports.0.to_string(), format!("127.0.0.1:{}", ports.1));
    let args = [
        ["--state", state.to_str().unwrap()],
        ["--inbox", inbox.to_str().unwrap()],
        ["--name", name],
        ["--kind", kind],
        ["--port", &port],
        ["--announce", "off"],
        ["--peer", &peer],
    ];
    let mut daemons = vec![world.spawn(&mesh, "system.mesh", &args.concat())];
    if let Some(script) = model {
        let socket = world.subdir("llm").join("llm.sock");
        model_service(&socket, script);
        let args = ["--socket", socket.to_str().unwrap()];
        daemons.push(world.spawn(&llm_bus, "system.llmbus", &args));
    }
    let agent = env!("CARGO_BIN_EXE_ultar-agentd");
    daemons.push(world.spawn(agent, "system.agentd", &[]));
    let shell = world.client("system.shell");
    Device {
        world,
        _daemons: daemons,
        shell,
        seen,
    }
}

/// Both on, paired with the code the computer shows; the computer lets the phone use its
/// model and ask its helper. Returns the computer's id as the phone knows it.
fn pair(phone: &Device, desk: &Device) -> String {
    let on = SetEnabled { enabled: true };
    assert_eq!(call(&desk.shell, &on, 10).unwrap(), Done { ok: true });
    assert_eq!(call(&phone.shell, &on, 10).unwrap(), Done { ok: true });
    let desk_id = phone.peer_called("Desk", false);
    let code = call(&desk.shell, &PairBegin, 10).unwrap().code;
    let join = PairJoin {
        device: desk_id.clone(),
        code,
    };
    assert_eq!(call(&phone.shell, &join, 30).unwrap(), Done { ok: true });
    let phone_id = desk.peer_called("Phone", true);
    let all = [
        Right::Handoff,
        Right::Files,
        Right::Clipboard,
        Right::Control,
        Right::Model,
    ];
    let grant = SetRights::of(&phone_id, all.to_vec());
    assert_eq!(call(&desk.shell, &grant, 10).unwrap(), Done { ok: true });
    desk_id
}

#[test]
fn a_phone_without_a_model_thinks_on_the_computer_and_lets_its_helper_act_there() {
    let script = Script::default();
    let (p, d) = (free_port(), free_port());
    let phone = device("both-phone", "Phone", "phone", (p, d), None);
    let desk = device("both-desk", "Desk", "desktop", (d, p), Some(&script));
    assert!(phone.world.subdir("state").exists());
    desk.brain(&Brain::Local);
    phone.brain(&Brain::Minimal);
    let desk_id = pair(&phone, &desk);
    // the computer lends its model: the phone's helper thinks there from now on
    phone.brain(&Brain::Mesh(desk_id.clone()));

    let say = |lines: &[String]| script.lock().unwrap().extend(lines.iter().cloned());
    say(&[
        r#"{"tool":"mute","args":{"on":"true"}}"#.to_owned(),
        r#"{"say":"It is quiet now."}"#.to_owned(),
    ]);
    let answer = phone.ask("please make it quiet");
    assert_eq!(answer.brain, Brain::Mesh(desk_id.clone()));
    assert_eq!(answer.steps.len(), 1);
    assert_eq!(answer.steps[0].outcome, StepOutcome::Done);
    assert_eq!(phone.took(), ["mute true"], "muted on the phone");
    assert!(desk.took().is_empty(), "nothing happened on the computer");

    // "on the computer: ...": the phone's helper asks the computer's helper
    say(&[
        format!(
            r#"{{"tool":"ask_device","args":{{"device":"{desk_id}","request":"open the files"}}}}"#
        ),
        r#"{"tool":"open_app","args":{"app":"org.ultar.files"}}"#.to_owned(),
        r#"{"say":"Files is open."}"#.to_owned(),
        r#"{"say":"Files is open on the computer."}"#.to_owned(),
    ]);
    let answer = phone.ask("open the files on the computer");
    let steps: Vec<_> = answer
        .steps
        .iter()
        .map(|s| (s.tool.as_str(), s.outcome))
        .collect();
    assert_eq!(steps, [("ask_device", StepOutcome::Done)]);
    assert_eq!(desk.took(), ["open files"], "opened on the computer");
    assert!(phone.took().is_empty());
    assert!(script.lock().unwrap().is_empty(), "every thought was used");
}
