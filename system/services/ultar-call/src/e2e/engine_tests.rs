use super::*;
use ed25519_dalek::{Signer as _, SigningKey};

struct Key(SigningKey);

impl Signer for Key {
    fn public(&self) -> Option<[u8; 32]> {
        Some(self.0.verifying_key().to_bytes())
    }
    fn sign(&self, m: &[u8]) -> Option<[u8; 64]> {
        Some(self.0.sign(m).to_bytes())
    }
}

fn engine(seed: u8) -> Engine {
    Engine::new(
        Box::new(Key(SigningKey::from_bytes(&[seed; 32]))),
        Pins::default(),
    )
}

fn cands(port: u16) -> Vec<SocketAddr> {
    vec![SocketAddr::from(([10, 0, 0, 1], port))]
}

/// What the server would deliver: `data` as text, `from` written by the server.
fn frame(out: &Out) -> Option<(String, Value)> {
    match out {
        Out::Send { to, data } => Some((to.clone(), json!({"type": "e2e", "data": data}))),
        _ => None,
    }
}

/// Delivers every message in `outs` to `to_engine`; returns what it answered.
fn deliver(outs: &[Out], from: &str, to: &mut Engine, port: u16) -> Vec<Out> {
    outs.iter()
        .filter_map(frame)
        .flat_map(|(_, v)| to.on_signal(from, &v, || cands(port)))
        .collect()
}

fn has(outs: &[Out], f: impl Fn(&Out) -> bool) -> bool {
    outs.iter().any(f)
}

#[test]
fn a_whole_call_and_the_server_reads_nothing_after_the_keys() {
    let (mut a, mut b) = (engine(1), engine(2));
    let hello = a.dial("B", true, cands(1));
    let reply = deliver(&hello, "A", &mut b, 2);
    let confirm = deliver(&reply, "B", &mut a, 1);
    assert!(has(&confirm, |o| matches!(
        o,
        Out::Secured { changed: false, .. }
    )));
    let ring = deliver(&confirm, "A", &mut b, 2);
    assert!(has(&ring, |o| matches!(
        o,
        Out::Incoming { video: true, .. }
    )));
    let safety_b = ring.iter().find_map(|o| match o {
        Out::Secured { safety, .. } => Some(safety.clone()),
        _ => None,
    });
    let safety_a = confirm.iter().find_map(|o| match o {
        Out::Secured { safety, .. } => Some(safety.clone()),
        _ => None,
    });
    assert_eq!(
        safety_a, safety_b,
        "both phones show the same safety number"
    );
    assert!(has(&deliver(&ring, "B", &mut a, 1), |o| matches!(
        o,
        Out::Ringing
    )));
    let accept = b.answer(cands(2));
    let Some(Out::Start(pb)) = accept.iter().find(|o| matches!(o, Out::Start(_))) else {
        panic!("no start")
    };
    assert_eq!(
        (pb.peer.clone(), pb.video, pb.role),
        (cands(1), true, Role::Callee)
    );
    let started = deliver(&accept, "B", &mut a, 1);
    let Some(Out::Start(pa)) = started.iter().find(|o| matches!(o, Out::Start(_))) else {
        panic!("no start")
    };
    assert_eq!(pa.peer, cands(2));
    // what the server saw after hello/reply: no word, no address
    for o in confirm.iter().chain(&ring).chain(&accept) {
        if let Out::Send { data, .. } = o {
            for secret in ["video", "10.0.0.1", "accept", "ring", "confirm"] {
                assert!(!data.contains(secret), "{secret} in {data}");
            }
        }
    }
    let bye = a.hangup();
    assert!(has(&deliver(&bye, "A", &mut b, 2), |o| matches!(
        o,
        Out::Ended("remote")
    )));
    assert!(!a.busy() && !b.busy());
}

#[test]
fn a_server_in_between_cannot_take_part() {
    let (mut a, mut b) = (engine(1), engine(2));
    let hello = a.dial("B", false, cands(1));
    // the server swaps the caller's ephemeral key for its own
    let (_, mut v) = frame(&hello[0]).unwrap();
    let mut d: Value = serde_json::from_str(v["data"].as_str().unwrap()).unwrap();
    d["ea"] = Value::from(b64(&Ephemeral::new().public));
    v["data"] = Value::from(d.to_string());
    let reply = b.on_signal("A", &v, || cands(2));
    let end = deliver(&reply, "B", &mut a, 1);
    assert!(
        has(&end, |o| matches!(o, Out::Ended("failed"))),
        "the caller sees the signature fail"
    );
    // a confirmation that does not come from the caller's key: the callee never rings
    let (mut a, mut b, mut m) = (engine(1), engine(2), engine(9));
    let hello = a.dial("B", false, cands(1));
    let reply = deliver(&hello, "A", &mut b, 2);
    let _ = deliver(&reply, "B", &mut a, 1);
    let forged = m.dial("B", false, cands(9));
    let ring = deliver(&forged, "A", &mut b, 2);
    assert!(
        !has(&ring, |o| matches!(o, Out::Incoming { .. })),
        "busy: a second hello is ignored"
    );
}

#[test]
fn a_changed_key_is_reported_and_unreachable_ends_the_call() {
    let mut a = engine(1);
    for (seed, changed) in [(2u8, false), (2, false), (3, true)] {
        let mut b = engine(seed);
        let hello = a.dial("B", false, cands(1));
        let reply = deliver(&hello, "A", &mut b, 2);
        let confirm = deliver(&reply, "B", &mut a, 1);
        assert!(has(
            &confirm,
            |o| matches!(o, Out::Secured { changed: c, .. } if *c == changed)
        ));
        a.hangup();
    }
    let _ = a.dial("Z", false, cands(1));
    let gone = a.on_signal("", &json!({"type": "unreachable", "to": "Z"}), Vec::new);
    assert!(has(&gone, |o| matches!(o, Out::Ended("unreachable"))));
    assert!(!a.busy());
}
