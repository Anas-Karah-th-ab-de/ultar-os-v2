//! The sound server's seam to the bus: the engine runs on a thread of its own (it must
//! never wait for the bus), the service loop hands it commands and takes its events.
//!
//! * `audio.capture` (voice control): a lease per caller - the caller asks again within
//!   [`LEASE`] or the microphone goes off, so a program that crashed never leaves it on;
//! * `audio.play`: a short clip, answered when it has been played out;
//! * `audio.mic`: the microphone as chunks of 20 ms at 16 kHz, while somebody leases it;
//! * `audio.mic_state`: who records, for the mark in the shell.

use crate::engine::{Engine, Event, ListenerSpec};
use crate::service::AudioService;
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use crate::conn::Rights;
use ultar_audio_stream::{MAX_RATE, MIN_RATE};
use ultar_bus::messages::audio::MicState;
use ultar_bus::messages::voice::{AudioDone, Capture, MicChunk, Play, MAX_CHUNK_SAMPLES, SAMPLE_RATE};
use ultar_bus::{Call, Identity, IncomingRequest, Policy};
use ultar_service::log::{Level, Record};
use ultar_svckit::{Bus, Poster};

/// How long a microphone request of the bus counts without being renewed.
pub const LEASE: Duration = Duration::from_secs(60);
/// Clips waiting to be played at most.
const MAX_PENDING: usize = 8;
/// How often the engine thread looks while it has something to do, and while it has not.
const BUSY: Duration = Duration::from_millis(5);
const IDLE: Duration = Duration::from_millis(20);

/// What the service tells the engine thread.
pub enum Cmd {
    /// Who has the microphone over the bus now.
    Capture(Vec<String>),
    /// `accessibility.mono_audio` changed.
    Mono(bool),
    /// Play a clip.
    Clip {
        /// The number the answer carries.
        request: u64,
        /// Sample rate of the clip.
        rate: u32,
        /// The clip (mono).
        samples: Vec<i16>,
    },
}

/// The rights of an identity, from the bus policy: `audio.play`, `audio.ring`,
/// `audio.call` and `audio.capture`.
pub fn rights_of(policy: &Policy, identity: &str) -> Rights {
    let Ok(id) = Identity::new(identity) else {
        return Rights::default();
    };
    let may = |method: &str| policy.may_call(&id, ultar_bus::messages::audio::SERVICE, method);
    Rights {
        play: may("play"),
        ring: may("ring"),
        call: may("call"),
        capture: may("capture"),
    }
}

/// The sockets of a list of identities: `identity` or `identity:gid`, below `dir`.
pub fn listeners(
    dir: &std::path::Path,
    list: &str,
    policy: &Policy,
) -> Result<Vec<ListenerSpec>, String> {
    let mut specs = Vec::new();
    for item in list.split(',').filter(|s| !s.is_empty()) {
        let (identity, group) = match item.split_once(':') {
            Some((i, g)) => (i, Some(g.parse::<u32>().map_err(|_| format!("group of {item}"))?)),
            None => (item, None),
        };
        Identity::new(identity).map_err(|_| format!("identity {identity}"))?;
        if specs.iter().any(|s: &ListenerSpec| s.identity == identity) {
            return Err(format!("identity {identity} twice"));
        }
        specs.push(ListenerSpec {
            path: dir.join(format!("{identity}.sock")),
            identity: identity.to_owned(),
            rights: rights_of(policy, identity),
            // the group of the program may connect, nobody else
            mode: Some(0o660),
            group,
        });
    }
    Ok(specs)
}

/// The sound server inside the service.
pub struct Sound {
    tx: Sender<Cmd>,
    rx: Option<Receiver<Cmd>>,
    engine: Option<Engine>,
    pending: HashMap<u64, IncomingRequest>,
    next: u64,
    leases: BTreeMap<String, Instant>,
    card: bool,
    mic: MicState,
}

impl Sound {
    /// A sound server around `engine` (not running yet).
    pub fn new(engine: Engine) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx: Some(rx),
            engine: Some(engine),
            pending: HashMap::new(),
            next: 1,
            leases: BTreeMap::new(),
            card: false,
            mic: MicState::default(),
        }
    }

    /// Starts the engine thread.
    pub fn start(&mut self, bus: &Bus<'_, AudioService>) {
        bus.retain(&self.mic);
        let (Some(engine), Some(rx)) = (self.engine.take(), self.rx.take()) else {
            return;
        };
        let poster = bus.poster();
        let started = std::thread::Builder::new()
            .name("audiod-engine".into())
            .spawn(move || engine_loop(engine, &rx, &poster));
        if started.is_err() {
            bus.log(Record::new(Level::Error, "audiod.engine_not_started"));
        }
    }

    fn send_users(&self) {
        let _ = self.tx.send(Cmd::Capture(self.leases.keys().cloned().collect()));
    }

    /// `audio.capture` of `caller`; `false` when there is no card.
    pub fn set_capture(&mut self, caller: &str, on: bool) -> bool {
        if on && !self.card {
            return false;
        }
        if on {
            self.leases.insert(caller.to_owned(), Instant::now() + LEASE);
        } else {
            self.leases.remove(caller);
        }
        self.send_users();
        self.card || !on
    }

    /// Ends the leases that were not renewed.
    pub fn expire(&mut self, now: Instant) {
        let before = self.leases.len();
        self.leases.retain(|_, until| *until > now);
        if self.leases.len() != before {
            self.send_users();
        }
    }

    /// Both channels in every speaker (`accessibility.mono_audio`).
    pub fn set_mono(&mut self, on: bool) {
        let _ = self.tx.send(Cmd::Mono(on));
    }

    /// Plays a clip as a prompt; the request is answered when it was played out.
    pub fn play(&mut self, request: IncomingRequest, play: Play) {
        let rate_ok = (MIN_RATE..=MAX_RATE).contains(&play.rate);
        if !self.card
            || !rate_ok
            || play.samples.is_empty()
            || play.samples.len() > MAX_CHUNK_SAMPLES
            || self.pending.len() >= MAX_PENDING
        {
            let _ = request.reply(&AudioDone { ok: false });
            return;
        }
        let id = self.next;
        self.next += 1;
        self.pending.insert(id, request);
        let _ = self.tx.send(Cmd::Clip {
            request: id,
            rate: play.rate,
            samples: play.samples,
        });
    }

    /// The microphone state to publish now, if it changed.
    pub fn mic(&self) -> &MicState {
        &self.mic
    }

    /// Handles what the engine reports.
    pub fn event(&mut self, ev: Event, bus: &Bus<'_, AudioService>) {
        match ev {
            Event::Mic(users) => {
                self.mic = MicState {
                    in_use: !users.is_empty(),
                    users,
                };
                bus.retain(&self.mic);
                bus.log(
                    Record::new(Level::Info, "audiod.mic")
                        .word("in_use", if self.mic.in_use { "yes" } else { "no" }),
                );
            }
            Event::Chunk(samples) => bus.publish(&MicChunk {
                rate: SAMPLE_RATE,
                samples,
            }),
            Event::ClipDone { request, ok } => {
                if let Some(r) = self.pending.remove(&request) {
                    let _ = r.reply(&AudioDone { ok });
                }
            }
            Event::Card(present) => {
                self.card = present;
                if !present {
                    self.leases.clear();
                }
                bus.log(Record::new(
                    Level::Info,
                    if present { "audiod.card" } else { "audiod.card_gone" },
                ));
            }
        }
    }
}

/// The method names the service answers for the sound server.
pub fn is_sound_call(method: &str) -> bool {
    method == Capture::METHOD || method == Play::METHOD
}

fn apply(eng: &mut Engine, cmd: Cmd) {
    match cmd {
        Cmd::Capture(users) => eng.set_bus_capture(users),
        Cmd::Mono(on) => eng.set_mono(on),
        Cmd::Clip {
            request,
            rate,
            samples,
        } => eng.play_clip(request, rate, &samples),
    }
}

fn engine_loop(mut eng: Engine, rx: &Receiver<Cmd>, poster: &Poster<AudioService>) {
    loop {
        let wait = if eng.idle() { IDLE } else { BUSY };
        match rx.recv_timeout(wait) {
            Ok(cmd) => apply(&mut eng, cmd),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while let Ok(cmd) = rx.try_recv() {
            apply(&mut eng, cmd);
        }
        eng.step(Instant::now());
        for ev in eng.drain_events() {
            let posted = poster.post(Box::new(move |s: &mut AudioService, bus| s.sound_event(ev, bus)));
            if !posted {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = r#"
[[rule]]
identity = "system.calld"
call = ["audio.call", "audio.capture"]

[[rule]]
identity = "system.voiced"
call = ["audio.capture", "audio.play"]

[[rule]]
identity = "system.mediad"
call = ["audio.play"]

[[rule]]
identity = "app.web"
call = ["audio.play"]

[[rule]]
identity = "app.mesh"
call = ["audio.play", "audio.ring"]
"#;

    #[test]
    fn rights_come_from_the_bus_policy_and_nobody_records_by_default() {
        let p = Policy::from_toml_str(POLICY).unwrap();
        let calld = rights_of(&p, "system.calld");
        assert!(calld.call && calld.capture && !calld.play && !calld.ring);
        let voiced = rights_of(&p, "system.voiced");
        assert!(voiced.capture && voiced.play && !voiced.call);
        let web = rights_of(&p, "app.web");
        assert!(web.play && !web.capture && !web.call && !web.ring);
        assert!(rights_of(&p, "app.mesh").ring);
        assert_eq!(rights_of(&p, "app.unknown"), Rights::default());
        assert_eq!(rights_of(&p, "not an identity"), Rights::default());
    }

    #[test]
    fn the_stream_list_is_checked() {
        let p = Policy::from_toml_str(POLICY).unwrap();
        let dir = std::path::Path::new("/run/ultar/audio");
        let l = listeners(dir, "system.calld:829,app.web", &p).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].group, Some(829));
        assert_eq!(l[0].mode, Some(0o660));
        assert_eq!(l[0].path, dir.join("system.calld.sock"));
        assert!(l[0].rights.call && !l[1].rights.call);
        assert!(listeners(dir, "system.calld,system.calld", &p).is_err());
        assert!(listeners(dir, "system.calld:x", &p).is_err());
        assert!(listeners(dir, "bad identity", &p).is_err());
        assert!(listeners(dir, "", &p).unwrap().is_empty());
    }
}
