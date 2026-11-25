//! [`CallBackend`] of the Ultar way, end to end: signalling socket, handshake ([`Engine`]),
//! UDP socket with relay ([`Net`]) and the running media ([`Media`]) – each call on a thread of
//! its own, the phone sees only [`CallEvent`]s.

use crate::e2e::engine::{Engine, Out, Plan, Signer};
use crate::e2e::media::{AudioIo, Media, MediaEvent, VideoIo};
use crate::e2e::net::{Net, TurnConfig};
use crate::e2e::pins::Pins;
use crate::e2e::signal::Signal;
use crate::{CallBackend, CallEvent, CallState, SignalAuth};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What the device lends a call: its key (through the account service), the relay's
/// credentials, microphone, speaker and camera.
pub trait Device: Send + Sync {
    /// The account server (`https://host`) while the person is signed in; none: no calls.
    fn server(&self) -> Option<String> {
        None
    }
    /// The device key's proof for the signalling socket.
    fn auth(&self) -> Option<Box<dyn SignalAuth>> {
        None
    }
    /// The device key's signer.
    fn signer(&self) -> Box<dyn Signer>;
    /// Short-lived TURN credentials (none: calls only within one network).
    fn turn(&self) -> Option<TurnConfig>;
    /// Microphone and speaker for one call.
    fn audio(&self) -> Box<dyn AudioIo>;
    /// Camera and picture of the other side for one video call (none: voice only).
    fn video(&self) -> Option<Box<dyn VideoIo>>;
    /// Where the pinned keys of other numbers are kept.
    fn pins(&self) -> Pins;
    /// Calls only through the relay: the other side never learns this device's addresses.
    fn relay_only(&self) -> bool {
        false
    }
}

enum Cmd {
    Register(String, Box<dyn SignalAuth>),
    Place(String, bool),
    Answer,
    Hangup,
}

/// The end-to-end call backend.
pub struct E2eCall {
    tx: Sender<Cmd>,
    events: Arc<Mutex<Vec<CallEvent>>>,
    state: Arc<Mutex<CallState>>,
    ready: Arc<AtomicBool>,
    number: Arc<Mutex<Option<String>>>,
    /// Microphone muted (read by the media every 20 ms).
    pub muted: Arc<AtomicBool>,
}

struct Running {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

struct Worker {
    device: Arc<dyn Device>,
    events: Arc<Mutex<Vec<CallEvent>>>,
    state: Arc<Mutex<CallState>>,
    ready: Arc<AtomicBool>,
    number: Arc<Mutex<Option<String>>>,
    muted: Arc<AtomicBool>,
    engine: Engine,
    net: Option<Net>,
    media: Option<Running>,
    media_events: (Sender<MediaEvent>, Receiver<MediaEvent>),
    peer: String,
    /// Signed in with the device's account (not registered by hand).
    via_device: bool,
}

impl E2eCall {
    /// A backend lending calls `device`.
    pub fn new(device: Arc<dyn Device>) -> Self {
        let (tx, rx) = mpsc::channel();
        let events = Arc::new(Mutex::new(Vec::new()));
        let state = Arc::new(Mutex::new(CallState::Idle));
        let ready = Arc::new(AtomicBool::new(false));
        let muted = Arc::new(AtomicBool::new(false));
        let number = Arc::new(Mutex::new(None));
        let mut w = Worker {
            engine: Engine::new(device.signer(), device.pins()),
            device,
            events: events.clone(),
            state: state.clone(),
            ready: ready.clone(),
            number: number.clone(),
            muted: muted.clone(),
            net: None,
            media: None,
            media_events: mpsc::channel(),
            peer: String::new(),
            via_device: false,
        };
        let _ = std::thread::Builder::new()
            .name("e2e-call".into())
            .spawn(move || w.run(&rx));
        Self {
            tx,
            events,
            state,
            ready,
            number,
            muted,
        }
    }
}

impl CallBackend for E2eCall {
    fn register(&mut self, server_base: &str, auth: Box<dyn SignalAuth>) {
        let _ = self.tx.send(Cmd::Register(server_base.to_owned(), auth));
    }
    fn place(&mut self, peer: &str, video: bool) {
        if let Ok(mut s) = self.state.lock() {
            *s = CallState::Dialing(peer.to_owned());
        }
        let _ = self.tx.send(Cmd::Place(peer.to_owned(), video));
    }
    fn answer(&mut self) {
        let _ = self.tx.send(Cmd::Answer);
    }
    fn hangup(&mut self) {
        let _ = self.tx.send(Cmd::Hangup);
    }
    fn poll(&mut self) -> Vec<CallEvent> {
        self.events
            .lock()
            .map(|mut e| std::mem::take(&mut *e))
            .unwrap_or_default()
    }
    fn state(&self) -> CallState {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(CallState::Idle)
    }
    fn ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }
    fn number(&self) -> Option<String> {
        self.number.lock().ok().and_then(|n| n.clone())
    }
}

impl Worker {
    fn event(&self, e: CallEvent) {
        if let Ok(mut v) = self.events.lock() {
            v.push(e);
        }
    }

    fn set(&self, s: CallState) {
        if let Ok(mut st) = self.state.lock() {
            *st = s;
        }
    }

    /// Where and how to sign in: given by `register`, else the device's account while signed in.
    #[allow(clippy::type_complexity)]
    fn login(
        &self,
        given: &mut Option<(String, Box<dyn SignalAuth>)>,
        rx: &Receiver<Cmd>,
    ) -> Option<(String, Box<dyn SignalAuth>, bool)> {
        loop {
            if let Some((s, a)) = given.take() {
                return Some((s, a, false));
            }
            if let (Some(s), Some(a)) = (self.device.server(), self.device.auth()) {
                return Some((s, a, true));
            }
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(Cmd::Register(s, a)) => *given = Some((s, a)),
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn run(&mut self, rx: &Receiver<Cmd>) {
        let mut given = None;
        let mut wait = 1;
        loop {
            let Some((server, auth, via_device)) = self.login(&mut given, rx) else {
                return;
            };
            self.via_device = via_device;
            match Signal::connect(&server, auth.as_ref()) {
                Ok(sig) => {
                    wait = 1;
                    self.ready.store(true, Ordering::Relaxed);
                    if let Ok(mut n) = self.number.lock() {
                        *n = Some(sig.number.clone());
                    }
                    if let Some(n) = recovery_notice(&sig.ready) {
                        self.event(n);
                    }
                    let gone = self.serve(sig, rx).is_none();
                    self.ready.store(false, Ordering::Relaxed);
                    if gone {
                        return; // the backend was dropped
                    }
                }
                Err(code) => self.event(CallEvent::Error(code)),
            }
            if !via_device {
                given = Some((server, auth)); // registered by hand: keep trying that server
            }
            for _ in 0..wait * 10 {
                if matches!(rx.try_recv(), Err(TryRecvError::Disconnected)) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            wait = (wait * 2).min(60);
        }
    }

    /// Serves one signalling connection; `None` when the backend is gone.
    fn serve(&mut self, mut sig: Signal, rx: &Receiver<Cmd>) -> Option<()> {
        loop {
            loop {
                match rx.try_recv() {
                    Ok(cmd) => self.command(cmd, &mut sig),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return None,
                }
            }
            if let Ok(e) = self.media_events.1.try_recv() {
                if e == MediaEvent::Lost {
                    let outs = self.engine.hangup();
                    self.apply(outs, &mut sig);
                    self.event(CallEvent::Error("media_lost".into()));
                }
            }
            match sig.poll() {
                Ok(Some(v)) => self.frame(&v, &mut sig),
                Ok(None) => {}
                Err(_) => return Some(()),
            }
            if self.signed_out() {
                let outs = self.engine.hangup();
                self.apply(outs, &mut sig);
                return Some(());
            }
        }
    }

    /// The person signed out on this device (a device that was registered by hand never is).
    fn signed_out(&self) -> bool {
        self.via_device && self.device.server().is_none()
    }

    fn cands(&mut self) -> Vec<SocketAddr> {
        if self.net.is_none() {
            self.net = Net::open(self.device.turn().as_ref()).ok();
            if let Some(n) = self.net.as_mut() {
                n.relay_only = self.device.relay_only();
            }
        }
        self.net.as_ref().map(Net::candidates).unwrap_or_default()
    }

    fn command(&mut self, cmd: Cmd, sig: &mut Signal) {
        let outs = match cmd {
            Cmd::Register(..) => Vec::new(),
            Cmd::Place(peer, video) => {
                self.peer.clone_from(&peer);
                let c = self.cands();
                self.engine.dial(&peer, video, c)
            }
            Cmd::Answer => {
                let c = self.cands();
                self.engine.answer(c)
            }
            Cmd::Hangup => self.engine.hangup(),
        };
        self.apply(outs, sig);
    }

    fn frame(&mut self, v: &Value, sig: &mut Signal) {
        let ty = v.get("type").and_then(Value::as_str).unwrap_or_default();
        if ty == "notice" {
            let field = |k: &str| {
                v.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .chars()
                    .take(64)
                    .collect()
            };
            return self.event(CallEvent::Notice {
                event: field("event"),
                device: field("device"),
            });
        }
        let from = v
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let mut cands = Vec::new();
        let needs = ty == "e2e" && !self.engine.busy();
        if needs {
            cands = self.cands();
        }
        let outs = self.engine.on_signal(&from, v, move || cands);
        if needs && !self.engine.busy() {
            self.net = None; // the hello was not answered: give the socket back
        }
        self.apply(outs, sig);
    }

    fn apply(&mut self, outs: Vec<Out>, sig: &mut Signal) {
        for o in outs {
            match o {
                Out::Send { to, data } => {
                    let _ = sig.send(&to, &data);
                }
                Out::Incoming { from, video } => {
                    self.peer.clone_from(&from);
                    self.set(CallState::Incoming(from.clone()));
                    self.event(CallEvent::IncomingCall {
                        number: from,
                        video,
                    });
                }
                Out::Ringing => self.event(CallEvent::Ringing),
                Out::Secured { safety, changed } => {
                    self.event(CallEvent::Secured { safety, changed })
                }
                Out::Start(plan) => self.start(*plan),
                Out::Ended(_) => self.end(),
            }
        }
    }

    fn start(&mut self, plan: Plan) {
        let Some(net) = self.net.take() else {
            return self.end();
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (device, muted, tx) = (
            self.device.clone(),
            self.muted.clone(),
            self.media_events.0.clone(),
        );
        let s = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut m = Media::new(net, plan.media, plan.peer, muted);
            if !m.find_way(&s) {
                let _ = tx.send(MediaEvent::Lost);
                return m.close();
            }
            let _ = tx.send(MediaEvent::Connected {
                relayed: m.relayed(),
            });
            let mut audio = device.audio();
            let mut video = if plan.video { device.video() } else { None };
            match video.as_mut() {
                Some(v) => m.run(audio.as_mut(), Some(v.as_mut()), &s, &tx),
                None => m.run(audio.as_mut(), None, &s, &tx),
            }
            m.close();
        });
        self.media = Some(Running { stop, thread });
        self.set(CallState::Active(self.peer.clone()));
        self.event(CallEvent::Answered);
    }

    fn end(&mut self) {
        if let Some(r) = self.media.take() {
            r.stop.store(true, Ordering::Relaxed);
            let _ = r.thread.join();
        }
        if let Some(mut n) = self.net.take() {
            n.close();
        }
        self.set(CallState::Ended);
        self.event(CallEvent::Ended);
    }
}

/// The pending recovery a `ready` frame reports, as the notice the device may have missed.
fn recovery_notice(ready: &Value) -> Option<CallEvent> {
    let r = ready.get("recovery").filter(|r| r.is_object())?;
    let device = r
        .get("old_device")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(CallEvent::Notice {
        event: "recovery_started".into(),
        device: device.chars().take(8).collect(),
    })
}

/// Microphone switch and output of Ultar calls on a device without a modem: the media read the
/// switch every 20 ms; the output is the sound card's (one output, `Earpiece` = the default output).
pub struct MuteRoute {
    muted: Arc<AtomicBool>,
}

impl MuteRoute {
    /// The route over this switch (see [`E2eCall::muted`]).
    pub fn new(muted: Arc<AtomicBool>) -> Self {
        Self { muted }
    }
}

impl crate::line::AudioRoute for MuteRoute {
    fn set_mute(&mut self, on: bool) -> Result<(), crate::line::LineError> {
        self.muted.store(on, Ordering::Relaxed);
        Ok(())
    }
    fn set_output(
        &mut self,
        output: ultar_bus::messages::call::Output,
    ) -> Result<(), crate::line::LineError> {
        if output == ultar_bus::messages::call::Output::Earpiece {
            Ok(())
        } else {
            Err(crate::line::LineError::Unsupported)
        }
    }
    fn outputs(&self) -> Vec<ultar_bus::messages::call::Output> {
        vec![ultar_bus::messages::call::Output::Earpiece]
    }
}
