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
}

fn dev(dir: &Path, name: &str, kind: DeviceKind, keys: Option<Keys>) -> Dev {
    let keys = keys.unwrap_or_else(|| Keys::generate().unwrap());
    let config = Config {
        name: name.to_owned(),
        kind,
        capabilities: default_capabilities(kind),
        apps: vec!["editor".to_owned()],
        inbox: dir.join(format!("inbox-{name}")),
        remote_view: false,
    };
    let trust = Trust::open(&dir.join(format!("trust-{name}")));
    let node = Node::new(config, &keys.public, trust);
    let (tx, rx) = mpsc::channel();
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let net = Net::start(keys.clone(), bind, None, tx.clone()).unwrap();
    Dev {
        rt: Runtime::new(node, tx),
        rx,
        net,
        host: Vec::new(),
        keys,
    }
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
                program: "sh".into(),
                args: vec!["-c".into(), format!("echo $(({i} * {i}))")],
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
