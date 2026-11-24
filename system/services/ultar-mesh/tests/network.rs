//! Two devices as two runtimes with real sockets and the real encrypted channel.
mod common;
use common::net::{addr, dev, dir, has, jobs_dir, linked, pair, paired, until};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::{Activity, DeviceKind, OfferState, Right, SendOutcome};
use ultar_mesh::compute::{Job, LOCAL};
use ultar_mesh::net::Net;
use ultar_mesh::node::Effect;

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
