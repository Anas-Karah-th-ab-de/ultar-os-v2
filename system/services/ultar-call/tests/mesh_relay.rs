//! Calls between the person's devices as the call service sees them: a real bus, the real
//! call service with a fake line and fake sound devices, and a stand-in for the local mesh
//! service that records what the call service asks of it and reports what a mesh would
//! (the two real services together are tested in `ultar-mesh/tests/calls_e2e.rs`).

use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
use base64::Engine;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use ultar_bus::messages::call::{Answer, CallState, Hangup, Others, Phase, Snapshot, Ultar, Way};
use ultar_bus::messages::mesh::{Capability, DeviceKind, Done, MeshState, Peer, Right};
use ultar_bus::messages::mesh_calls::{
    CallAudio, CallAudioIn, CallEnd, CallIncoming, CallMeshMute, CallOffer, CallOffered, CallPhase,
    CallProgress, MeshCallState,
};
use ultar_bus::{Broker, Client, ClientOptions, FixedIdentity, Identity, Policy};
use ultar_call::fake::{Command, FakeAudio, FakeControl, FakeLine, ManualClock};
use ultar_call::mesh_relay::{MeshRelay, Taps};
use ultar_call::mesh_tap::{FakeTap, FakeTapControl, FRAME};
use ultar_call::service::Service;
use ultar_call::session::Session;

const WAIT: Duration = Duration::from_secs(10);

/// What the stand-in mesh got asked.
#[derive(Clone, Debug, PartialEq)]
enum Asked {
    Offer(String, String),
    End(u64),
    Audio(String, u64, u64, i16),
}

#[derive(Default)]
struct Log {
    asked: Vec<Asked>,
    next: u64,
    /// Devices that take an offer (others answer id 0).
    takes: Vec<String>,
}

struct Rig {
    _broker: Broker,
    driver: Client,
    app: Client,
    line: FakeControl,
    log: Arc<Mutex<Log>>,
    ultar: FakeTapControl,
    cellular: FakeTapControl,
    guest: FakeTapControl,
    stop: Arc<AtomicBool>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for w in self.workers.drain(..) {
            w.join().unwrap();
        }
    }
}

fn peer(id: &str, paired: bool, reachable: bool, call_right: bool) -> Peer {
    Peer {
        id: id.into(),
        name: format!("dev-{id}"),
        kind: DeviceKind::Desktop,
        capabilities: vec![Capability::AudioIn, Capability::AudioOut],
        paired,
        reachable,
        rights: if call_right {
            vec![Right::Handoff, Right::Call]
        } else {
            vec![Right::Handoff]
        },
    }
}

fn state(peers: Vec<Peer>) -> MeshState {
    MeshState {
        enabled: true,
        id: "phone".into(),
        name: "Phone".into(),
        peers,
    }
}

fn rig(tag: &str, with_cellular_tap: bool) -> Rig {
    let path = std::env::temp_dir().join(format!("umesh-relay-{}-{tag}.sock", std::process::id()));
    let auth = Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
    let broker = Broker::builder(Policy::allow_all())
        .listen(&path, auth)
        .start()
        .unwrap();
    let connect = |name: &str| Client::connect(&path, ClientOptions::named(name)).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::new();

    // The stand-in mesh service.
    let log = Arc::new(Mutex::new(Log::default()));
    let mesh = connect("mesh");
    let endpoint = mesh.register("mesh").unwrap();
    let (flag, record) = (stop.clone(), log.clone());
    workers.push(thread::spawn(move || {
        // The connection lives as long as the stand-in does (the service ends with it).
        let _mesh = mesh;
        while !flag.load(Ordering::SeqCst) {
            let Ok(request) = endpoint.recv_timeout(Duration::from_millis(5)) else {
                continue;
            };
            let record = record.clone();
            match request.method() {
                "call_offer" => {
                    let _ = request.respond::<CallOffer>(|c| {
                        let mut log = record.lock().unwrap();
                        log.asked
                            .push(Asked::Offer(c.device.clone(), c.caller.clone()));
                        if log.takes.contains(&c.device) {
                            log.next += 1;
                            Ok(CallOffered {
                                id: 100 + log.next,
                                device: c.device,
                            })
                        } else {
                            Ok(CallOffered {
                                id: 0,
                                device: String::new(),
                            })
                        }
                    });
                }
                "call_end" => {
                    let _ = request.respond::<CallEnd>(|c| {
                        record.lock().unwrap().asked.push(Asked::End(c.id));
                        Ok(Done { ok: true })
                    });
                }
                "call_audio" => {
                    let _ = request.respond::<CallAudioIn>(|c| {
                        let bytes = B64.decode(&c.pcm).unwrap();
                        assert_eq!(bytes.len(), FRAME * 2, "a frame is exactly 20 ms");
                        let first = i16::from_le_bytes([bytes[0], bytes[1]]);
                        record
                            .lock()
                            .unwrap()
                            .asked
                            .push(Asked::Audio(c.device, c.id, c.seq, first));
                        Ok(Done { ok: true })
                    });
                }
                _ => {}
            }
        }
    }));

    // The call service with a fake line and fake sound devices.
    let (fake, line) = FakeLine::new(&[Way::Ultar, Way::Cellular(1)]);
    line.set_ultar(Ultar::Ready, Some("UOS-1111-2222"));
    let session = Session::new(
        vec![Box::new(fake)],
        Box::new(FakeAudio::new(&[
            ultar_bus::messages::call::Output::Earpiece,
        ])),
        Box::new(ManualClock::default()),
    );
    let (ultar_tap, ultar) = FakeTap::new();
    let (cellular_tap, cellular) = FakeTap::new();
    let (guest_tap, guest) = FakeTap::new();
    let taps = Taps {
        ultar: Some(Box::new(ultar_tap)),
        cellular: with_cellular_tap.then(|| Box::new(cellular_tap) as _),
        guest: Some(Box::new(guest_tap)),
    };
    let calld = connect("calld");
    let relay = MeshRelay::new(connect("calld-mesh"), taps);
    let mut service = Service::new(calld, session, Duration::from_millis(5))
        .unwrap()
        .with_mesh_relay(relay);
    let flag = stop.clone();
    workers.push(thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            service.step().unwrap();
        }
    }));

    Rig {
        _broker: broker,
        driver: connect("driver"),
        app: connect("phone-app"),
        line,
        log,
        ultar,
        cellular,
        guest,
        stop,
        workers,
    }
}

/// Waits until `check` holds.
fn until(what: &str, mut check: impl FnMut() -> bool) {
    let start = Instant::now();
    while !check() {
        assert!(start.elapsed() < WAIT, "timed out: {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

impl Rig {
    fn asked(&self) -> Vec<Asked> {
        self.log.lock().unwrap().asked.clone()
    }

    fn takes(&self, devices: &[&str]) {
        self.log.lock().unwrap().takes = devices.iter().map(|d| (*d).to_owned()).collect();
    }

    fn calls(&self) -> CallState {
        self.app.call(&Snapshot, WAIT).unwrap()
    }

    fn progress(&self, id: u64, device: &str, phase: CallPhase) {
        let p = CallProgress {
            id,
            device: device.into(),
            phase,
        };
        self.driver.publish(&p).unwrap();
    }

    fn offers(&self) -> Vec<(String, String)> {
        self.asked()
            .into_iter()
            .filter_map(|a| match a {
                Asked::Offer(d, c) => Some((d, c)),
                _ => None,
            })
            .collect()
    }

    fn ended(&self) -> Vec<u64> {
        self.asked()
            .into_iter()
            .filter_map(|a| match a {
                Asked::End(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    fn sent(&self) -> Vec<(String, u64, u64, i16)> {
        self.asked()
            .into_iter()
            .filter_map(|a| match a {
                Asked::Audio(d, id, seq, v) => Some((d, id, seq, v)),
                _ => None,
            })
            .collect()
    }

    fn pcm(&self, id: u64, seq: u64, mark: i16) {
        let bytes: Vec<u8> = vec![mark; FRAME]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let a = CallAudio {
            id,
            seq,
            pcm: B64.encode(bytes),
            device: "desk".into(),
        };
        self.driver.publish(&a).unwrap();
    }
}

fn ring_ultar(r: &Rig) -> u32 {
    r.line.ring(Way::Ultar, "UOS-5555-6666", Some("Samer"));
    until("the call rings", || r.calls().calls.len() == 1);
    r.calls().calls[0].id
}

#[test]
fn a_ring_is_offered_only_to_devices_that_may_carry_it() {
    let r = rig("offer", true);
    r.takes(&["desk", "tablet"]);
    r.driver
        .publish(&state(vec![
            peer("desk", true, true, true),
            peer("tablet", true, true, true),
            peer("norights", true, true, false),
            peer("away", true, false, true),
            peer("stranger", false, true, true),
        ]))
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    ring_ultar(&r);
    until("two offers", || r.offers().len() == 2);
    thread::sleep(Duration::from_millis(100));
    let offers = r.offers();
    assert_eq!(
        offers,
        vec![
            ("desk".to_owned(), "Samer".to_owned()),
            ("tablet".to_owned(), "Samer".to_owned())
        ],
        "only what the phone shows, only to paired, reachable devices with the right"
    );
}

#[test]
fn nothing_is_offered_when_the_mesh_is_off_or_a_mobile_call_cannot_be_carried() {
    let r = rig("noconsent", false);
    r.takes(&["desk"]);
    let mut off = state(vec![peer("desk", true, true, true)]);
    off.enabled = false;
    r.driver.publish(&off).unwrap();
    thread::sleep(Duration::from_millis(100));
    let first = ring_ultar(&r);
    thread::sleep(Duration::from_millis(200));
    assert!(r.offers().is_empty(), "the mesh is off: nothing rings");
    r.app.call(&Hangup { id: first }, WAIT).unwrap();
    // Mesh on, but a mobile call without a modem voice tap would be a silent call.
    r.driver
        .publish(&state(vec![peer("desk", true, true, true)]))
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    r.line.ring(Way::Cellular(1), "0933", None);
    until("the mobile call rings", || r.calls().calls.len() == 1);
    thread::sleep(Duration::from_millis(200));
    assert!(
        r.offers().is_empty(),
        "no voice tap for the SIM: not offered"
    );
}

#[test]
fn the_first_device_that_answers_takes_the_call_and_the_voice_flows_in_order() {
    let r = rig("first", true);
    r.takes(&["desk", "tablet"]);
    r.driver
        .publish(&state(vec![
            peer("desk", true, true, true),
            peer("tablet", true, true, true),
        ]))
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    let call = ring_ultar(&r);
    until("two offers", || r.offers().len() == 2);
    let (desk_offer, tablet_offer) = (101, 102);
    // The tablet is first; the desk answers a moment later and is turned away.
    r.progress(tablet_offer, "tablet", CallPhase::Accepted);
    until("the phone answered", || {
        r.calls().calls[0].phase == Phase::Active
    });
    assert!(r
        .line
        .commands()
        .iter()
        .any(|c| matches!(c, Command::Answer(_))));
    until("the other offer ended", || r.ended().contains(&desk_offer));
    r.progress(desk_offer, "desk", CallPhase::Accepted);
    until("the late answer is refused", || {
        r.ended().iter().filter(|e| **e == desk_offer).count() >= 2
    });
    let answers = r
        .line
        .commands()
        .iter()
        .filter(|c| matches!(c, Command::Answer(_)))
        .count();
    assert_eq!(answers, 1, "a call is answered once");
    assert_eq!(r.calls().calls[0].id, call);
    assert_eq!(r.ultar.calls().0, 1, "the voice is bridged");

    // The microphone of the call goes to the tablet in order.
    for mark in 1..=4i16 {
        r.ultar.speak(vec![mark; FRAME]);
    }
    until("four frames sent", || r.sent().len() == 4);
    let sent = r.sent();
    assert!(sent
        .iter()
        .all(|(d, id, ..)| d == "tablet" && *id == tablet_offer));
    assert_eq!(
        sent.iter()
            .map(|(_, _, seq, v)| (*seq, *v))
            .collect::<Vec<_>>(),
        [(0, 1), (1, 2), (2, 3), (3, 4)]
    );

    // The tablet's voice (one frame lost, one late) plays in order into the call.
    for (seq, mark) in [(0u64, 11i16), (1, 12), (3, 14), (4, 15), (5, 16), (6, 17)] {
        r.pcm(tablet_offer, seq, mark);
    }
    r.pcm(999, 0, 99); // another call's voice is not played
    until("the voice was heard", || {
        r.ultar
            .heard()
            .iter()
            .filter(|f| f.iter().any(|s| *s != 0))
            .count()
            >= 6
    });
    let heard: Vec<i16> = r
        .ultar
        .heard()
        .iter()
        .filter(|f| f[0] != 0)
        .map(|f| f[0])
        .collect();
    assert_eq!(heard[..2], [11, 12]);
    assert_eq!(
        heard[2], 6,
        "the lost frame repeats the last one at half volume"
    );
    assert!(!heard.contains(&99));
}

#[test]
fn hanging_up_on_either_side_ends_the_call_everywhere() {
    // The desk hangs up: the phone's call ends.
    let r = rig("desk-hangs-up", true);
    r.takes(&["desk"]);
    r.driver
        .publish(&state(vec![peer("desk", true, true, true)]))
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    ring_ultar(&r);
    until("offer", || r.offers().len() == 1);
    r.progress(101, "desk", CallPhase::Accepted);
    until("active", || {
        r.calls().calls.first().map(|c| c.phase) == Some(Phase::Active)
    });
    r.progress(101, "desk", CallPhase::Ended);
    until("the phone's call ended", || r.calls().calls.is_empty());
    assert!(r
        .line
        .commands()
        .iter()
        .any(|c| matches!(c, Command::Release(_))));
    until("the tap let go", || r.ultar.calls() == (1, 1));

    // The phone hangs up: the desk is told.
    let r = rig("phone-hangs-up", true);
    r.takes(&["desk"]);
    r.driver
        .publish(&state(vec![peer("desk", true, true, true)]))
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    let call = ring_ultar(&r);
    until("offer", || r.offers().len() == 1);
    r.progress(101, "desk", CallPhase::Accepted);
    until("active", || {
        r.calls().calls.first().map(|c| c.phase) == Some(Phase::Active)
    });
    r.app.call(&Hangup { id: call }, WAIT).unwrap();
    until("the desk was told", || r.ended().contains(&101));
    until("the tap let go", || r.ultar.calls() == (1, 1));
}

#[test]
fn answering_or_declining_on_the_phone_ends_the_offers() {
    let r = rig("local", true);
    r.takes(&["desk"]);
    r.driver
        .publish(&state(vec![peer("desk", true, true, true)]))
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    let call = ring_ultar(&r);
    until("offer", || r.offers().len() == 1);
    r.app
        .call(
            &Answer {
                id: call,
                others: Others::Hold,
            },
            WAIT,
        )
        .unwrap();
    until("the desk's ring ended", || r.ended() == vec![101]);
    // The call stays on the phone alone: no bridge.
    assert_eq!(r.ultar.calls(), (0, 0));
}

#[test]
fn a_lost_link_leaves_the_call_on_the_phone() {
    let r = rig("link", true);
    r.takes(&["desk"]);
    r.driver
        .publish(&state(vec![peer("desk", true, true, true)]))
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    ring_ultar(&r);
    until("offer", || r.offers().len() == 1);
    r.progress(101, "desk", CallPhase::Accepted);
    until("bridged", || r.ultar.calls().0 == 1);
    r.progress(101, "desk", CallPhase::Failed);
    until("the tap let go", || r.ultar.calls() == (1, 1));
    assert_eq!(
        r.calls().calls[0].phase,
        Phase::Active,
        "the phone keeps its call"
    );
}

#[test]
fn a_guest_carries_the_voice_mutes_and_lets_go() {
    let r = rig("guest", true);
    let state_sub = r.app.subscribe::<MeshCallState>().unwrap();
    // The phone offers a call here; the person answered (the shell's `mesh.call_answer`).
    r.driver
        .publish(&CallIncoming {
            id: 7,
            from: "phone".into(),
            from_name: "Phone".into(),
            caller: "Samer".into(),
            video: false,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    r.progress(7, "phone", CallPhase::Accepted);
    until("the guest tap began", || r.guest.calls().0 == 1);
    let mut led = None;
    until("the call is announced", || {
        while let Ok(Some(s)) = state_sub.try_recv() {
            led = Some(s);
        }
        led.as_ref().is_some_and(|s| s.id == Some(7))
    });
    assert_eq!(
        led.as_ref().map(|s| (s.device.as_str(), s.muted)),
        Some(("phone", false))
    );

    // The desk's microphone goes to the phone.
    r.guest.speak(vec![5; FRAME]);
    until("a frame sent", || r.sent().len() == 1);
    assert_eq!(r.sent()[0], ("phone".to_owned(), 7, 0, 5));
    // Mute: the microphone is read but nothing goes out.
    let ok = r.app.call(&CallMeshMute { on: true }, WAIT).unwrap();
    assert!(ok.ok);
    until("muted is announced", || {
        while let Ok(Some(s)) = state_sub.try_recv() {
            led = Some(s);
        }
        led.as_ref().is_some_and(|s| s.muted)
    });
    r.guest.speak(vec![6; FRAME]);
    thread::sleep(Duration::from_millis(150));
    assert_eq!(r.sent().len(), 1, "muted: nothing leaves the desk");
    r.app.call(&CallMeshMute { on: false }, WAIT).unwrap();
    r.guest.speak(vec![7; FRAME]);
    until("unmuted", || r.sent().len() == 2);
    // The phone's voice plays on the desk's speaker.
    for seq in 0..5u64 {
        r.pcm(7, seq, 20 + seq as i16);
    }
    until("heard", || {
        r.guest.heard().iter().filter(|f| f[0] != 0).count() >= 5
    });
    // The phone hangs up.
    r.progress(7, "phone", CallPhase::Ended);
    until("the card was let go", || r.guest.calls() == (1, 1));
    until("the call is over", || {
        while let Ok(Some(s)) = state_sub.try_recv() {
            led = Some(s);
        }
        led.as_ref().is_some_and(|s| s.id.is_none())
    });
    // Without a call there is nothing to mute.
    let none = r.app.call(&CallMeshMute { on: true }, WAIT).unwrap();
    assert!(!none.ok);
    assert_eq!(r.cellular.calls(), (0, 0));
}
