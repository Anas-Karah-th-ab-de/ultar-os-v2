//! The flow of "continue on another device" with both ends as the people use them: a phone
//! and a desktop, each with its own bus and a real `ultar-mesh` service, paired with a code.
//! On the phone the app card is held and swiped up (the gesture of the recent apps), the
//! phone shell's model sends and shows "waiting"; on the desktop the system dialog's real
//! view asks and its Accept button is pressed; the launcher of the desktop then opens the
//! same app at the same place, and the phone learns it.

use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;
use ultar_bus::messages::launcher::{AppStatus, InstanceState, List, ListReply, Start, StartReply};
use ultar_bus::messages::mesh::{
    Activity, Done, HandoffProgress, HandoffSend, MeshState, OfferState, PairBegin, PairJoin,
    SendOutcome, SetActivity, SetEnabled,
};
use ultar_bus::messages::mesh_dialog::{AskHandoff, HandoffDecision};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.mesh"
register = ["mesh"]
publish = ["mesh.state", "mesh.handoff_incoming", "mesh.handoff_progress", "mesh.clipboard", "mesh.received"]
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

/// What the desktop's system dialog does with the question: the real view is built, the
/// Accept button is pressed like a finger would, and the dialog model's answer is returned.
fn desktop_dialog(question: AskHandoff) -> bool {
    use ultar_desktop::dialog::{Answer, DialogEffect, Dialogs, Question};
    use ultar_desktop::view::{Look, Target};
    use ultar_ui::event::{ActionKind, Event, PointerButton};
    let mut dialogs = Dialogs::default();
    dialogs.ask(1, Question::Handoff(question), 0);
    let lang = ultar_i18n::Lang::Ar;
    let tr = ultar_desktop::i18n::translator(lang).expect("catalogs");
    let look = Look::new(lang, ultar_ui::tokens::ThemeKind::Dark);
    let mut view = ultar_desktop::dialog::view::build(&dialogs, 0, look, &tr)
        .expect("builds")
        .expect("a question");
    view.ui.layout();
    let accept = view.node(&Target::AllowOnce).expect("an Accept button");
    let pos = view.ui.rect(accept).expect("laid out").center();
    let mut actions = Vec::new();
    for event in [
        Event::PointerMove { pos },
        Event::PointerDown {
            pos,
            button: PointerButton::Primary,
        },
        Event::PointerUp {
            pos,
            button: PointerButton::Primary,
        },
    ] {
        actions.extend(view.ui.handle(event).actions);
    }
    let pressed = actions.iter().any(|a| {
        view.target(a.node) == Some(&Target::AllowOnce) && a.kind == ActionKind::Activated
    });
    assert!(pressed, "the click reached Accept");
    let effects = dialogs.answer(Answer::Handoff(true));
    effects
        .iter()
        .any(|e| matches!(e, DialogEffect::Reply(1, Answer::Handoff(true))))
}

#[test]
fn a_phone_continues_an_app_on_a_desktop_at_the_same_place() {
    use ultar_mobile::app::{apply_click, drag_click, Effect};
    use ultar_mobile::click::Click;
    use ultar_mobile::model::mesh::Act;
    use ultar_mobile::Mobile;
    use ultar_ui::geom::Point;

    let (port_a, port_b) = (free_port(), free_port());
    let b = device("flow-b", "Desk", port_b, None);
    let a = device("flow-a", "Phone", port_a, Some(port_b));
    // The desktop's system dialog: the real model and view answer.
    let (tx, asked) = mpsc::channel();
    b.world
        .serve("system.sysdialog", "sysdialog", move |request| {
            if let Ok(question) = request.decode::<AskHandoff>() {
                let _ = tx.send(question.clone());
                let _ = request.reply(&HandoffDecision {
                    accept: desktop_dialog(question),
                });
            }
        });
    let progress_a = a.shell.subscribe::<HandoffProgress>().unwrap();
    let state_a = a.shell.subscribe::<MeshState>().unwrap();
    let peer = switch_on(&a, &b);
    let code = call(&b.shell, &PairBegin).unwrap().code;
    let join = PairJoin {
        device: peer.clone(),
        code,
    };
    assert_eq!(call(&a.shell, &join).unwrap(), Done { ok: true });

    // The editor on the phone says what it shows: a document, at line 3.
    let editor = a.world.client("app.editor");
    let source = a.world.subdir("home").join("letter.txt");
    std::fs::write(&source, vec![b'x'; 200_000]).unwrap();
    let set = SetActivity {
        activity: Some(Activity {
            app: "editor".into(),
            title: "letter.txt".into(),
            uri: Some(format!("file://{}#line=3", source.display())),
            file: Some(source.to_str().unwrap().to_owned()),
        }),
    };
    assert_eq!(call(&editor, &set).unwrap(), Done { ok: true });

    // The phone's model knows the mesh from its state topic: a paired device is connected.
    let mut phone = Mobile::default();
    assert!(wait_until(WAIT, || {
        while let Ok(Some(state)) = state_a.try_recv() {
            phone.mesh.state = Some(state);
        }
        phone.mesh.can_send()
    }));
    // The card of the app in the recent apps is held and swiped up.
    let swipe = Point { x: 0.0, y: -120.0 };
    let click = drag_click(Some(&Click::App("editor".into())), swipe).expect("the gesture");
    assert_eq!(click, Click::MeshSend("editor".into()));
    let mut effects = Vec::new();
    apply_click(&mut phone, click, 8, &mut effects);
    let Some(Effect::MeshSend { app, device }) = effects.pop() else {
        panic!("{effects:?}");
    };
    // What the phone's shell does with it (`Link::send`).
    let shell_set = SetActivity {
        activity: Some(Activity {
            app: app.clone(),
            title: "letter.txt".into(),
            uri: None,
            file: None,
        }),
    };
    let _ = call(&a.shell, &shell_set);
    let outcome = call(&a.shell, &HandoffSend { target: device }).unwrap();
    assert!(
        matches!(outcome, SendOutcome::Offered { .. }),
        "{outcome:?}"
    );
    phone.mesh.sent(&app, outcome, 0);
    let waiting = phone.mesh.notice_card(0).expect("a notice");
    assert_eq!(waiting.key, "mobile.mesh.waiting");
    assert!(
        matches!(waiting.act, Some(Act::Cancel(_))),
        "Cancel is offered"
    );

    // The desktop asked its person, who pressed Accept.
    let question = asked.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (question.from_name.as_str(), question.title.as_str()),
        ("Phone", "letter.txt")
    );
    // Its launcher opens the same app at the same place, from the received file.
    let start = b.started.recv_timeout(WAIT).unwrap();
    let arrived = b.world.subdir("inbox").join("letter.txt");
    assert_eq!(start.app, "editor");
    assert_eq!(
        start.uri.unwrap(),
        format!("file://{}#line=3", arrived.display())
    );
    assert_eq!(std::fs::read(&arrived).unwrap().len(), 200_000);
    // The phone learns it: the waiting notice becomes "opened on the other device".
    let mut seen = Vec::new();
    assert!(wait_until(WAIT, || {
        while let Ok(Some(p)) = progress_a.try_recv() {
            phone.mesh.progress(&p, 1_000);
            seen.push(p.state);
        }
        seen.contains(&OfferState::Opened)
    }));
    assert_eq!(phone.mesh.notice_at(1_000), Some("mobile.mesh.opened"));
}
