//! Devices as runtimes with real sockets and the real encrypted channel.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::{DeviceKind, HandoffProgress, OfferState};
use ultar_mesh::device::default_capabilities;
use ultar_mesh::net::{Net, NetEvent};
use ultar_mesh::node::{Config, Effect, Node};
use ultar_mesh::runtime::Runtime;
use ultar_mesh::secure::Keys;
use ultar_mesh::trust::Trust;

pub struct Dev {
    pub rt: Runtime,
    pub rx: mpsc::Receiver<NetEvent>,
    pub net: Net,
    pub host: Vec<Effect>,
    pub keys: Keys,
    pub tx: mpsc::Sender<NetEvent>,
}

pub fn dev(dir: &Path, name: &str, kind: DeviceKind, keys: Option<Keys>) -> Dev {
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
pub fn jobs_dir(dir: &Path) -> PathBuf {
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

pub fn addr(d: &Dev) -> SocketAddr {
    format!("127.0.0.1:{}", d.net.port()).parse().unwrap()
}

/// Lets the devices work until `done` holds (at most 20 s).
pub fn until(devs: &mut [&mut Dev], what: &str, done: impl Fn(&[&mut Dev]) -> bool) {
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

pub fn paired(d: &Dev, other: &Dev) -> bool {
    let id = other.rt.node.id().to_owned();
    d.rt.node
        .state()
        .peers
        .iter()
        .any(|p| p.id == id && p.paired && p.reachable)
}

pub fn linked(d: &Dev, other: &Dev) -> bool {
    let id = other.rt.node.id().to_owned();
    d.rt.node
        .state()
        .peers
        .iter()
        .any(|p| p.id == id && p.reachable && !p.name.is_empty())
}

pub fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mesh-net-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn has(d: &Dev, state: OfferState) -> bool {
    d.host
        .iter()
        .any(|e| matches!(e, Effect::Progress(HandoffProgress { state: s, .. }) if *s == state))
}

pub fn pair(phone: &mut Dev, pc: &mut Dev) {
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
