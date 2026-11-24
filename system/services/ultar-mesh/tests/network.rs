//! Two devices as two runtimes with real sockets and the real encrypted channel.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::{
    Activity, DeviceKind, HandoffProgress, OfferState, Right, SendOutcome,
};
use ultar_mesh::compute::{Job, LOCAL};
use ultar_mesh::device::default_capabilities;
use ultar_mesh::net::{Net, NetEvent};
use ultar_mesh::node::{Config, Effect, Node};
use ultar_mesh::runtime::Runtime;
use ultar_mesh::secure::Keys;
use ultar_mesh::trust::Trust;

struct Dev {
    rt: Runtime,
    rx: mpsc::Receiver<NetEvent>,
    net: Net,
    host: Vec<Effect>,
    keys: Keys,
    tx: mpsc::Sender<NetEvent>,
}

fn dev(dir: &Path, name: &str, kind: DeviceKind, keys: Option<Keys>) -> Dev {
    let keys = keys.unwrap_or_else(|| Keys::generate().unwrap());
    let config = Config {
        name: name.to_owned(),
        kind,
        capabilities: default_capabilities(kind),
        apps: vec!["editor".to_owned()],
        inbox: dir.join(format!("inbox-{name}")),
        share: vec![dir.to_owned()],
        remote_view: false,
    };
    let trust = Trust::open(&dir.join(format!("trust-{name}")));
    let node = Node::new(config, &keys.public, trust);
    let (tx, rx) = mpsc::channel();
    let tx2 = tx.clone();
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let net = Net::start(keys.clone(), bind, None, tx.clone()).unwrap();
    let mut rt = Runtime::new(node, tx);
    rt.set_jobs(Some(jobs_dir(dir)));
    Dev {
        rt,
        rx,
        net,
        host: Vec::new(),
        keys,
        tx: tx2,
    }
}

/// The one program this device lends its power to.
fn jobs_dir(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let jobs = dir.join("jobs");
    std::fs::create_dir_all(&jobs).unwrap();
    let script = jobs.join("square");
    if !script.exists() {
        std::fs::write(&script, "#!/bin/sh\necho $(($1 * $1))\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    jobs
}

fn addr(d: &Dev) -> SocketAddr {
    format!("127.0.0.1:{}", d.net.port()).parse().unwrap()
}

/// Lets the devices work until `done` holds (at most 20 s).
fn until(devs: &mut [&mut Dev], what: &str, done: impl Fn(&[&mut Dev]) -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        for d in devs.iter_mut() {
            let now = Instant::now();
            while let Ok(event) = d.rx.try_recv() {
                let left = d.rt.handle(event, now);
                d.host.extend(left);
            }
            let left = d.rt.tick(now);
            d.host.extend(left);
        }
        if done(devs) {
            return;
        }
        assert!(Instant::now() < end, "timeout: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn paired(d: &Dev, other: &Dev) -> bool {
    let id = other.rt.node.id().to_owned();
    d.rt.node
        .state()
        .peers
        .iter()
        .any(|p| p.id == id && p.paired && p.reachable)
}

fn linked(d: &Dev, other: &Dev) -> bool {
    let id = other.rt.node.id().to_owned();
    d.rt.node
        .state()
        .peers
        .iter()
        .any(|p| p.id == id && p.reachable && !p.name.is_empty())
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mesh-net-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn has(d: &Dev, state: OfferState) -> bool {
    d.host
        .iter()
        .any(|e| matches!(e, Effect::Progress(HandoffProgress { state: s, .. }) if *s == state))
}

fn pair(phone: &mut Dev, pc: &mut Dev) {
    phone.net.connect(addr(pc));
    until(&mut [phone, pc], "link", |d| {
        linked(d[0], d[1]) && linked(d[1], d[0])
    });
    let code = pc.rt.node.pair_begin(Instant::now()).code;
    let pc_id = pc.rt.node.id().to_owned();
    let (ok, effects) = phone.rt.node.pair_join(&pc_id, &code);
    assert!(ok);
    phone.rt.apply(effects);
    until(&mut [phone, pc], "pairing", |d| {
        paired(d[0], d[1]) && paired(d[1], d[0])
    });
}

#[test]
fn a_document_continues_on_the_other_device_over_the_network() {
    let d = dir("handoff");
    let mut phone = dev(&d, "phone", DeviceKind::Phone, None);
    let mut pc = dev(&d, "pc", DeviceKind::Desktop, None);
    pair(&mut phone, &mut pc);

    let content: Vec<u8> = (0..1_500_000u32).map(|i| (i % 253) as u8).collect();
    let source = d.join("report.txt");
    std::fs::write(&source, &content).unwrap();
    phone.rt.node.set_activity(Some(Activity {
        app: "editor".into(),
        title: "report.txt".into(),
        uri: Some(format!("file://{}#line=40", source.display())),
        file: Some(source.to_str().unwrap().to_owned()),
    }));
    let (outcome, effects) = phone.rt.node.send(None, Instant::now());
    assert!(
        matches!(outcome, SendOutcome::Offered { .. }),
        "{outcome:?}"
    );
    phone.rt.apply(effects);
    until(&mut [&mut phone, &mut pc], "asked", |d| {
        d[1].host.iter().any(|e| matches!(e, Effect::Incoming(_)))
    });
    let offer = pc.host.iter().find_map(|e| match e {
        Effect::Incoming(i) => Some(i.offer),
        _ => None,
    });
    assert!(
        !d.join("inbox-pc").exists(),
        "nothing arrives before the person accepts"
    );
    let (ok, effects) = pc.rt.node.answer(offer.unwrap(), true, Instant::now());
    assert!(ok);
    let left = pc.rt.apply(effects);
    pc.host.extend(left);
    until(&mut [&mut phone, &mut pc], "arrived", |d| {
        d[1].host.iter().any(|e| matches!(e, Effect::Open { .. }))
    });
    let (offer, app, uri) = pc
        .host
        .iter()
        .find_map(|e| match e {
            Effect::Open { offer, app, uri } => Some((*offer, app.clone(), uri.clone().unwrap())),
            _ => None,
        })
        .unwrap();
    let arrived = d.join("inbox-pc").join("report.txt");
    assert_eq!(app, "editor");
    assert_eq!(uri, format!("file://{}#line=40", arrived.display()));
    assert_eq!(std::fs::read(&arrived).unwrap(), content);
    let effects = pc.rt.node.opened(offer, true);
    pc.rt.apply(effects);
    until(&mut [&mut phone, &mut pc], "opened", |d| {
        has(d[0], OfferState::Opened)
    });
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn pairing_survives_a_restart_and_a_switched_off_device_is_unreachable() {
    let d = dir("restart");
    let mut phone = dev(&d, "phone", DeviceKind::Phone, None);
    let mut pc = dev(&d, "pc", DeviceKind::Desktop, None);
    pair(&mut phone, &mut pc);
    let effects = pc.rt.set_enabled(false);
    pc.host.extend(effects);
    pc.net.stop();
    until(&mut [&mut phone], "gone", |d| {
        d[0].rt
            .node
            .state()
            .peers
            .iter()
            .all(|p| p.paired && !p.reachable)
    });
    // The computer starts again with the same key and the same trust file: no code needed.
    let keys = pc.keys.clone();
    drop(pc);
    let mut pc = dev(&d, "pc", DeviceKind::Desktop, Some(keys));
    phone.net.connect(addr(&pc));
    until(&mut [&mut phone, &mut pc], "back", |d| {
        paired(d[0], d[1]) && paired(d[1], d[0])
    });
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn jobs_run_on_the_other_device_only_with_its_permission() {
    let d = dir("compute");
    let mut phone = dev(&d, "phone", DeviceKind::Phone, None);
    let mut pc = dev(&d, "pc", DeviceKind::Desktop, None);
    pair(&mut phone, &mut pc);
    let jobs = |tag: &str| -> Vec<Job> {
        (0..6)
            .map(|i| Job {
                id: format!("{tag}{i}"),
                program: "square".into(),
                args: vec![i.to_string()],
            })
            .collect()
    };
    // Not allowed yet: the computer refuses, the phone does the work itself.
    assert!(phone.rt.compute(jobs("a")));
    assert!(!phone.rt.compute(jobs("x")), "one run at a time");
    let mut results = None;
    let end = Instant::now() + Duration::from_secs(60);
    while results.is_none() {
        until(&mut [&mut phone, &mut pc], "step", |_| true);
        results = phone.rt.computed();
        assert!(Instant::now() < end, "timeout: refused run");
        std::thread::sleep(Duration::from_millis(20));
    }
    let results = results.unwrap();
    assert_eq!(results.len(), 6);
    assert!(results.iter().all(|r| r.by == LOCAL), "{results:?}");
    // Allowed: the computer does it.
    let phone_id = phone.rt.node.id().to_owned();
    let mut rights = ultar_mesh::trust::DEFAULT_RIGHTS.to_vec();
    rights.push(Right::Compute);
    assert!(pc.rt.node.set_rights(&phone_id, &rights).0);
    assert!(pc.rt.lends_to(&phone_id));
    assert!(phone.rt.compute(jobs("b")));
    let mut results = None;
    let end = Instant::now() + Duration::from_secs(60);
    while results.is_none() {
        until(&mut [&mut phone, &mut pc], "step", |_| true);
        results = phone.rt.computed();
        assert!(Instant::now() < end, "timeout: lent run");
        std::thread::sleep(Duration::from_millis(20));
    }
    let results = results.unwrap();
    let outputs: Vec<&str> = results.iter().map(|r| r.output.trim()).collect();
    assert_eq!(outputs, ["0", "1", "4", "9", "16", "25"]);
    assert!(
        results.iter().all(|r| r.by == pc.rt.node.id()),
        "{results:?}"
    );
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn a_program_outside_the_jobs_folder_never_runs() {
    use ultar_mesh::runtime::job_program;
    let d = dir("jobs");
    let jobs = jobs_dir(&d);
    assert!(job_program(Some(&jobs), "square").is_some());
    for bad in [
        "sh",
        "/bin/sh",
        "../square",
        "./square",
        "",
        ".hidden",
        "a b",
    ] {
        assert!(job_program(Some(&jobs), bad).is_none(), "{bad}");
    }
    assert!(job_program(None, "square").is_none());
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn two_devices_find_each_other_by_their_announcements() {
    use std::net::UdpSocket;
    use ultar_mesh::net::Announcing;
    let d = dir("announce");
    let port = |_: u8| {
        UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let (udp_a, udp_b) = (port(0), port(1));
    let mut a = dev(&d, "a", DeviceKind::Phone, None);
    let mut b = dev(&d, "b", DeviceKind::Desktop, None);
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let to = |udp: u16| -> SocketAddr { format!("127.0.0.1:{udp}").parse().unwrap() };
    let (ta, tb) = (a.tx.clone(), b.tx.clone());
    a.net = Net::start(
        a.keys.clone(),
        bind,
        Some(Announcing {
            port: udp_a,
            to: to(udp_b),
        }),
        ta,
    )
    .unwrap();
    b.net = Net::start(
        b.keys.clone(),
        bind,
        Some(Announcing {
            port: udp_b,
            to: to(udp_a),
        }),
        tb,
    )
    .unwrap();
    // Nobody connects by hand: the announcements do it, and only one channel is opened.
    until(&mut [&mut a, &mut b], "found", |d| {
        linked(d[0], d[1]) && linked(d[1], d[0])
    });
    assert_eq!(a.rt.node.state().peers.len(), 1);
    std::fs::remove_dir_all(d).unwrap();
}

mod stand_in {
    //! A relay that follows the server's rules: frames go to the device whose key the
    //! sender names, with the sender's key put in its place; a wrong token is refused.
    use std::collections::HashMap;
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tungstenite::Message;

    type Table = Arc<Mutex<HashMap<String, mpsc::Sender<Message>>>>;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn announce(table: &Table) {
        let table = table.lock().unwrap();
        let list: Vec<_> = table
            .keys()
            .map(|pk| serde_json::json!({ "pk": pk }))
            .collect();
        let text = serde_json::json!({ "typ": "geraete", "liste": list }).to_string();
        for tx in table.values() {
            let _ = tx.send(Message::Text(text.clone()));
        }
    }

    fn forward(table: &Table, own: &str, mut frame: Vec<u8>) -> Option<Message> {
        let target = hex(&frame[1..33]);
        let raw: Vec<u8> = (0..32)
            .map(|i| u8::from_str_radix(&own[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        let back = frame.clone();
        frame[1..33].copy_from_slice(&raw);
        let sent = table
            .lock()
            .unwrap()
            .get(&target)
            .is_some_and(|tx| tx.send(Message::Binary(frame)).is_ok());
        (!sent).then(|| {
            let mut unreachable = back[..41].to_vec();
            unreachable[0] = 4;
            Message::Binary(unreachable)
        })
    }

    fn client(table: Table, stream: TcpStream) {
        let Ok(mut socket) = tungstenite::accept(stream) else {
            return;
        };
        let Ok(Message::Text(login)) = socket.read() else {
            return;
        };
        let login: serde_json::Value = serde_json::from_str(&login).unwrap();
        if login["token"] != "good" {
            let _ = socket.send(Message::Text(
                r#"{"typ":"fehler","text":"anmeldung"}"#.into(),
            ));
            return;
        }
        let own = login["pk"].as_str().unwrap().to_owned();
        let (tx, rx) = mpsc::channel();
        tx.send(Message::Text(r#"{"typ":"bereit"}"#.into()))
            .unwrap();
        table.lock().unwrap().insert(own.clone(), tx);
        announce(&table);
        socket
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        loop {
            for message in rx.try_iter() {
                if socket.send(message).is_err() {
                    break;
                }
            }
            match socket.read() {
                Ok(Message::Binary(frame)) if frame.len() >= 41 => {
                    if let Some(answer) = forward(&table, &own, frame) {
                        let _ = socket.send(answer);
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
                _ => {}
            }
        }
        table.lock().unwrap().remove(&own);
        announce(&table);
    }

    /// Starts the relay; its address.
    pub fn start() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/v1/verbund/relais", listener.local_addr().unwrap());
        let table = Table::default();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let table = table.clone();
                std::thread::spawn(move || client(table, stream));
            }
        });
        url
    }
}

fn login(url: &str, token: &str, name: &str) -> ultar_mesh::relay::Login {
    ultar_mesh::relay::Login {
        url: url.to_owned(),
        token: token.to_owned(),
        name: name.to_owned(),
        kind: "desktop".to_owned(),
    }
}

#[test]
fn devices_in_different_networks_meet_through_the_relay_and_hand_a_file_over() {
    let d = dir("relay");
    let url = stand_in::start();
    let mut phone = dev(&d, "phone", DeviceKind::Phone, None);
    let mut pc = dev(&d, "pc", DeviceKind::Desktop, None);
    let mut stranger = dev(&d, "stranger", DeviceKind::Desktop, None);
    phone.net.use_relay(login(&url, "good", "phone"));
    pc.net.use_relay(login(&url, "good", "pc"));
    stranger.net.use_relay(login(&url, "stolen", "stranger"));
    // Nobody knows anybody's address: the relay is the only way.
    until(&mut [&mut phone, &mut pc, &mut stranger], "met", |d| {
        linked(d[0], d[1]) && linked(d[1], d[0])
    });
    assert!(
        stranger.rt.node.state().peers.is_empty(),
        "a wrong token meets nobody"
    );
    // Met is not trusted: the code is still needed.
    assert!(!paired(&phone, &pc));
    let code = pc.rt.node.pair_begin(Instant::now()).code;
    let pc_id = pc.rt.node.id().to_owned();
    let (_, effects) = phone.rt.node.pair_join(&pc_id, &code);
    phone.rt.apply(effects);
    until(&mut [&mut phone, &mut pc], "paired", |d| {
        paired(d[0], d[1]) && paired(d[1], d[0])
    });

    let content: Vec<u8> = (0..400_000u32).map(|i| (i % 241) as u8).collect();
    let source = d.join("far.bin");
    std::fs::write(&source, &content).unwrap();
    let (outcome, effects) =
        phone
            .rt
            .node
            .send_file(&pc_id, source.to_str().unwrap(), Instant::now());
    assert!(
        matches!(outcome, SendOutcome::Offered { .. }),
        "{outcome:?}"
    );
    phone.rt.apply(effects);
    until(&mut [&mut phone, &mut pc], "asked", |d| {
        d[1].host.iter().any(|e| matches!(e, Effect::Incoming(_)))
    });
    let offer = pc.host.iter().find_map(|e| match e {
        Effect::Incoming(i) => Some(i.offer),
        _ => None,
    });
    let (_, effects) = pc.rt.node.answer(offer.unwrap(), true, Instant::now());
    let left = pc.rt.apply(effects);
    pc.host.extend(left);
    until(&mut [&mut phone, &mut pc], "arrived", |d| {
        d[1].host.iter().any(|e| matches!(e, Effect::Open { .. }))
    });
    assert_eq!(
        std::fs::read(d.join("inbox-pc").join("far.bin")).unwrap(),
        content
    );
    std::fs::remove_dir_all(d).unwrap();
}
