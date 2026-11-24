//! Two real `ultar-mesh` services, each on its own bus as on two devices: switched on by
//! the settings, paired with a code, an activity handed over by the shell, accepted on
//! the other device and opened there through the launcher.

use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;
use ultar_bus::messages::launcher::{AppStatus, InstanceState, List, ListReply, Start, StartReply};
use ultar_bus::messages::mesh::{
    Activity, Done, HandoffAnswer, HandoffIncoming, HandoffProgress, HandoffSend, MeshState,
    OfferState, PairBegin, PairJoin, SendOutcome, SetActivity, SetEnabled, Stage,
};
use ultar_bus::messages::mesh_dialog::{AskHandoff, HandoffDecision};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.mesh"
register = ["mesh"]
publish = ["mesh.state", "mesh.handoff_incoming", "mesh.handoff_progress", "mesh.clipboard"]
call = ["launcher.list", "launcher.start", "sysdialog.handoff", "llm.complete"]
subscribe = ["llm.state"]

[[rule]]
identity = "system.llm"
register = ["llm"]
publish = ["llm.state"]

[[rule]]
identity = "system.sysdialog"
register = ["sysdialog"]

[[rule]]
identity = "system.launcher"
register = ["launcher"]

[[rule]]
identity = "system.shell"
call = ["mesh.*"]
subscribe = ["mesh.*"]

[[rule]]
identity = "app.*"
call = ["mesh.set_activity"]
"#;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Device {
    world: World,
    _daemon: Daemon,
    shell: Client,
    started: mpsc::Receiver<Start>,
}

fn launcher(world: &World) -> mpsc::Receiver<Start> {
    let (tx, started) = mpsc::channel();
    world.serve("system.launcher", "launcher", move |request| {
        if request.method() == List::METHOD {
            let apps = ["editor", "browser", "files"]
                .iter()
                .map(|app| AppStatus {
                    app: (*app).to_owned(),
                    uid: 0,
                    profile: String::new(),
                    version: String::new(),
                    instances: Vec::new(),
                })
                .collect();
            let _ = request.reply(&ListReply { apps });
        } else if let Ok(start) = request.decode::<Start>() {
            let _ = tx.send(start);
            let _ = request.reply(&StartReply {
                state: InstanceState::Running,
                pid: 1,
                already: false,
                opened: true,
            });
        }
    });
    started
}

fn device(tag: &str, name: &str, port: u16, peer: Option<u16>) -> Device {
    let identities = [
        "system.mesh",
        "system.launcher",
        "system.sysdialog",
        "system.llm",
        "system.shell",
        "app.editor",
        "app.browser",
    ];
    let world = World::new(tag, POLICY, &identities);
    let started = launcher(&world);
    let state = world.subdir("state");
    let inbox = world.subdir("inbox");
    let port = port.to_string();
    let peer = peer.map(|p| format!("127.0.0.1:{p}"));
    let home = world.subdir("home");
    let mut args = vec!["--state", state.to_str().unwrap()];
    args.extend(["--share", home.to_str().unwrap()]);
    args.extend(["--inbox", inbox.to_str().unwrap(), "--name", name]);
    args.extend(["--kind", "desktop", "--port", &port, "--announce", "off"]);
    if let Some(peer) = &peer {
        args.extend(["--peer", peer]);
    }
    let daemon = world.spawn(env!("CARGO_BIN_EXE_ultar-mesh"), "system.mesh", &args);
    let shell = world.client("system.shell");
    Device {
        world,
        _daemon: daemon,
        shell,
        started,
    }
}

/// Switches both on; the id under which `a` sees `b`.
fn switch_on(a: &Device, b: &Device) -> String {
    let state_a = a.shell.subscribe::<MeshState>().unwrap();
    // Off after start: nothing listens, nobody is seen.
    let first = state_a.recv_timeout(WAIT).unwrap();
    assert!(!first.enabled && first.peers.is_empty());
    let on = SetEnabled { enabled: true };
    assert_eq!(call(&b.shell, &on).unwrap(), Done { ok: true });
    assert_eq!(call(&a.shell, &on).unwrap(), Done { ok: true });
    let mut peer = None;
    assert!(wait_until(WAIT, || {
        while let Ok(Some(state)) = state_a.try_recv() {
            let desk = state.peers.iter().find(|p| p.name == "Desk");
            peer = desk.map(|p| p.id.clone());
        }
        peer.is_some()
    }));
    peer.unwrap()
}

/// The editor of `a` reports a document; what an app may not do is checked on the way.
fn report_document(a: &Device) -> std::path::PathBuf {
    let app = a.world.client("app.editor");
    let source = a.world.subdir("home").join("letter.txt");
    std::fs::write(&source, vec![b'x'; 300_000]).unwrap();
    let set = SetActivity {
        activity: Some(Activity {
            app: "editor".into(),
            title: "letter.txt".into(),
            uri: Some(format!("file://{}#line=3", source.display())),
            file: Some(source.to_str().unwrap().to_owned()),
        }),
    };
    assert_eq!(call(&app, &set).unwrap(), Done { ok: true });
    // An app cannot speak for another app, nor send anything itself.
    let other = a.world.client("app.browser");
    assert_eq!(call(&other, &set).unwrap(), Done { ok: false });
    assert!(call(&app, &HandoffSend { target: None }).is_err());
    source
}

fn pair_and_send(a: &Device, b: &Device, peer: &str) {
    // Not paired yet: nowhere to hand it to.
    let send = HandoffSend { target: None };
    assert_eq!(call(&a.shell, &send).unwrap(), SendOutcome::NoTarget);
    let code = call(&b.shell, &PairBegin).unwrap().code;
    let join = PairJoin {
        device: peer.to_owned(),
        code,
    };
    assert_eq!(call(&a.shell, &join).unwrap(), Done { ok: true });
    let mut outcome = SendOutcome::NoTarget;
    assert!(wait_until(WAIT, || {
        outcome = call(&a.shell, &send).unwrap();
        matches!(outcome, SendOutcome::Offered { .. })
    }));
    let SendOutcome::Offered { target, .. } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!((target.device.as_str(), target.stage), (peer, Stage::State));
}

#[test]
fn two_services_pair_and_hand_a_document_over() {
    let (port_a, port_b) = (free_port(), free_port());
    let b = device("mesh-b", "Desk", port_b, None);
    let a = device("mesh-a", "Phone", port_a, Some(port_b));
    let incoming_b = b.shell.subscribe::<HandoffIncoming>().unwrap();
    let progress_a = a.shell.subscribe::<HandoffProgress>().unwrap();
    let peer = switch_on(&a, &b);
    report_document(&a);
    pair_and_send(&a, &b, &peer);

    // The other device asks its person; nothing was opened, nothing was written.
    let asked = incoming_b.recv_timeout(WAIT).unwrap();
    assert_eq!(asked.from_name, "Phone");
    assert_eq!(asked.title, "letter.txt");
    assert!(b.started.try_recv().is_err());
    let inbox = b.world.subdir("inbox");
    assert_eq!(std::fs::read_dir(&inbox).unwrap().count(), 0);
    let answer = HandoffAnswer {
        offer: asked.offer,
        accept: true,
    };
    assert_eq!(call(&b.shell, &answer).unwrap(), Done { ok: true });

    // Accepted: the launcher of the other device opens the same app at the same place.
    let start = b.started.recv_timeout(WAIT).unwrap();
    let arrived = inbox.join("letter.txt");
    assert_eq!(start.app, "editor");
    let place = format!("file://{}#line=3", arrived.display());
    assert_eq!(start.uri.unwrap(), place);
    assert_eq!(std::fs::read(&arrived).unwrap().len(), 300_000);
    let mut seen = Vec::new();
    assert!(wait_until(WAIT, || {
        while let Ok(Some(p)) = progress_a.try_recv() {
            seen.push(p.state);
        }
        seen.contains(&OfferState::Opened)
    }));
    let all = [OfferState::Asked, OfferState::Accepted, OfferState::Opened];
    assert_eq!(seen, all);
}

/// The system dialog of a device: answers every question with `accept` and records it.
fn system_dialog(d: &Device, accept: bool) -> mpsc::Receiver<AskHandoff> {
    let (tx, asked) = mpsc::channel();
    d.world
        .serve("system.sysdialog", "sysdialog", move |request| {
            if let Ok(question) = request.decode::<AskHandoff>() {
                let _ = tx.send(question);
                let _ = request.reply(&HandoffDecision { accept });
            }
        });
    asked
}

#[test]
fn the_system_dialog_asks_and_its_answer_decides() {
    let (port_a, port_b) = (free_port(), free_port());
    let b = device("meshd-b", "Desk", port_b, None);
    let a = device("meshd-a", "Phone", port_a, Some(port_b));
    let asked = system_dialog(&b, true);
    let progress_a = a.shell.subscribe::<HandoffProgress>().unwrap();
    let peer = switch_on(&a, &b);
    report_document(&a);
    pair_and_send(&a, &b, &peer);
    // The person is asked in the trusted dialog with everything it shows.
    let question = asked.recv_timeout(WAIT).unwrap();
    assert_eq!(question.from_name, "Phone");
    assert_eq!(question.title, "letter.txt");
    assert_eq!(question.app, "editor");
    assert_eq!(question.size, Some(300_000));
    assert_eq!(question.stage, Stage::State);
    // Accepted there: the launcher opens it, the sender learns it.
    let start = b.started.recv_timeout(WAIT).unwrap();
    assert_eq!(start.app, "editor");
    let mut seen = Vec::new();
    assert!(wait_until(WAIT, || {
        while let Ok(Some(p)) = progress_a.try_recv() {
            seen.push(p.state);
        }
        seen.contains(&OfferState::Opened)
    }));
}

#[test]
fn a_no_in_the_system_dialog_keeps_everything_out() {
    let (port_a, port_b) = (free_port(), free_port());
    let b = device("meshn-b", "Desk", port_b, None);
    let a = device("meshn-a", "Phone", port_a, Some(port_b));
    let asked = system_dialog(&b, false);
    let progress_a = a.shell.subscribe::<HandoffProgress>().unwrap();
    let peer = switch_on(&a, &b);
    report_document(&a);
    pair_and_send(&a, &b, &peer);
    asked.recv_timeout(WAIT).unwrap();
    let mut seen = Vec::new();
    assert!(wait_until(WAIT, || {
        while let Ok(Some(p)) = progress_a.try_recv() {
            seen.push(p.state);
        }
        seen.contains(&OfferState::Declined)
    }));
    assert!(b.started.try_recv().is_err(), "nothing was opened");
    let inbox = b.world.subdir("inbox");
    assert_eq!(
        std::fs::read_dir(inbox).unwrap().count(),
        0,
        "nothing was stored"
    );
}

/// The model of a device: announces itself and answers with the prompt turned around.
fn model(d: &Device) -> Client {
    use ultar_bus::messages::llm::{Complete, Completion, LlmState, ModelClass};
    d.world.serve("system.llm", "llm", |request| {
        if let Ok(ask) = request.decode::<Complete>() {
            let text = ask.prompt.chars().rev().collect();
            let _ = request.reply(&Completion { ok: true, text });
        }
    });
    let llm = d.world.client("system.llm");
    let state = LlmState {
        ready: true,
        model: "test".into(),
        class: ModelClass::Medium,
    };
    llm.publish(&state).unwrap();
    llm
}

#[test]
fn a_device_without_a_model_lets_a_paired_device_think_once_it_may() {
    use ultar_bus::messages::mesh::{Right, SetRights};
    use ultar_bus::messages::mesh_model::{Models, Think};
    let (port_a, port_b) = (free_port(), free_port());
    let b = device("meshm-b", "Desk", port_b, None);
    let a = device("meshm-a", "Phone", port_a, Some(port_b));
    let _llm = model(&b);
    let peer = switch_on(&a, &b);
    report_document(&a);
    pair_and_send(&a, &b, &peer);
    // The phone sees that the desk has a model…
    let mut lenders = Vec::new();
    assert!(wait_until(WAIT, || {
        lenders = call(&a.shell, &Models).unwrap().lenders;
        !lenders.is_empty()
    }));
    assert_eq!(lenders[0].name, "Desk");
    let think = Think {
        device: None,
        system: "rules".into(),
        prompt: "abc".into(),
        max_tokens: 16,
    };
    // …but may not use it before the desk allows it.
    assert!(!call(&a.shell, &think).unwrap().ok);
    let state_b = b.shell.subscribe::<MeshState>().unwrap();
    let phone = state_b.recv_timeout(WAIT).unwrap().peers[0].id.clone();
    let rights = vec![Right::Handoff, Right::Files, Right::Clipboard, Right::Model];
    let grant = SetRights {
        device: phone,
        rights,
    };
    assert_eq!(call(&b.shell, &grant).unwrap(), Done { ok: true });
    let thought = call(&a.shell, &think).unwrap();
    assert!(thought.ok);
    assert_eq!((thought.text.as_str(), thought.by), ("cba", peer));
}
