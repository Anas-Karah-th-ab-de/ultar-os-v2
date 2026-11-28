//! The engine of the sound server: the one holder of the card. It accepts the streams of
//! the programs, mixes what they play, keeps the card's queue short and full, and fans the
//! microphone out to everyone who may record. No threads, no bus: [`Engine::step`] does one
//! round of everything and takes the time as an argument, so the tests drive it with a
//! clock of their own.

use crate::capture::mono_of;
use crate::conn::{read_some, Conn, Greeted, Phase, Rights};
use crate::device::Device;
use crate::mix::Mix;
use crate::resample::{to_i16, Resampler};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ultar_audio_stream::{Class, Status, CARD_RATE};
use ultar_service::peercred::peer_credentials;
use ultar_service::socket::{bind_listener_for, ensure_parent};

/// Frames mixed at once (5 ms).
pub const PERIOD: usize = 240;
/// Frames the card's queue is kept at (10 ms): the latency the card adds.
pub const QUEUE: u64 = 480;
/// The queue a card that does not take more is assumed to hold (200 ms).
const FULL: u64 = 9600;
/// Streams open at the same time at most.
pub const MAX_STREAMS: usize = 16;
/// Connections (with those still sending their hello) at most.
const MAX_CONNS: usize = 32;
/// Longest time one step accounts for (after a stop of the process the clock does not
/// run away).
const MAX_STEP_FRAMES: u64 = CARD_RATE as u64;
/// How long a missing card waits before it is looked for again.
pub const REOPEN: Duration = Duration::from_secs(2);
/// Samples of one chunk of the microphone for the bus (20 ms at 16 kHz).
pub const CHUNK: usize = 320;

/// One socket programs connect to; the socket is the identity.
pub struct ListenerSpec {
    /// Where the socket is made.
    pub path: PathBuf,
    /// The identity of whoever connects (a bus identity such as `system.calld`).
    pub identity: String,
    /// What that identity may do.
    pub rights: Rights,
    /// The one user id that is this identity: the kernel's peer credentials of every
    /// connection must say it, otherwise the connection is closed. The socket itself may be
    /// open to everybody; being allowed to connect is not being the identity.
    pub uid: u32,
    /// File mode of the socket.
    pub mode: Option<u32>,
}

struct Lis {
    listener: UnixListener,
    identity: String,
    rights: Rights,
    uid: u32,
}

/// What the engine tells the service around it.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// The identities that record now (sorted, each once).
    Mic(Vec<String>),
    /// 20 ms of microphone at 16 kHz for the bus topic `audio.mic`.
    Chunk(Vec<i16>),
    /// A clip of the bus is played out (`ok`) or could not be played.
    ClipDone {
        /// The request it belongs to.
        request: u64,
        /// It was played.
        ok: bool,
    },
    /// The card appeared (`true`) or went.
    Card(bool),
}

/// Counters for the log and the tests.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    /// Streams opened.
    pub opened: u64,
    /// Streams refused for a missing right.
    pub denied: u64,
    /// Connections closed because the peer's user id is not the identity's.
    pub wrong_peer: u64,
    /// Times the card was lost.
    pub card_lost: u64,
}

/// A clip of the bus on its way into the mixer.
struct Clip {
    request: u64,
    rest: VecDeque<(f32, f32)>,
}

/// Opens the card.
pub type Opener = Box<dyn FnMut() -> io::Result<Box<dyn Device>> + Send>;

/// The engine.
pub struct Engine {
    failed: Vec<String>,
    listeners: Vec<Lis>,
    conns: Vec<Conn>,
    mix: Mix,
    opener: Opener,
    device: Option<Box<dyn Device>>,
    next_open: Option<Instant>,
    last: Option<Instant>,
    frac: f64,
    queued: u64,
    /// The mono sound in the card's queue, oldest first: the far end of the echo canceller.
    reference: VecDeque<f32>,
    /// `accessibility.mono_audio`: both channels in every speaker.
    mono: bool,
    mic16: Resampler,
    far16: Resampler,
    bus_users: Vec<String>,
    bus_acc: Vec<i16>,
    fresh: bool,
    clips: HashMap<u64, Clip>,
    /// Streams the mixer finished while mixing, to be taken away in `finish`.
    finished: Vec<u64>,
    next_id: u64,
    events: Vec<Event>,
    mic_now: Vec<String>,
    stats: Stats,
}

impl Engine {
    /// Binds the sockets of `specs`; the card is looked for with `opener`.
    pub fn new(specs: Vec<ListenerSpec>, opener: Opener) -> io::Result<Engine> {
        let mut listeners = Vec::new();
        let mut failed = Vec::new();
        for s in specs {
            ensure_parent(&s.path, 0o755)?;
            // one socket that cannot be made must not take the sound of everybody else
            // with it: it is reported and left out
            let bound = bind_listener_for(&s.path, s.mode, None)
                .map_err(|e| io::Error::other(e.to_string()))
                .and_then(|l| l.set_nonblocking(true).map(|()| l));
            match bound {
                Ok(listener) => listeners.push(Lis {
                    listener,
                    identity: s.identity,
                    rights: s.rights,
                    uid: s.uid,
                }),
                Err(_) => failed.push(s.identity),
            }
        }
        Ok(Engine {
            failed,
            listeners,
            conns: Vec::new(),
            mix: Mix::new(),
            opener,
            device: None,
            next_open: None,
            last: None,
            frac: 0.0,
            queued: 0,
            reference: VecDeque::new(),
            mono: false,
            mic16: Resampler::new(CARD_RATE, 16_000),
            far16: Resampler::new(CARD_RATE, 16_000),
            bus_users: Vec::new(),
            bus_acc: Vec::new(),
            fresh: true,
            clips: HashMap::new(),
            finished: Vec::new(),
            next_id: 1,
            events: Vec::new(),
            mic_now: Vec::new(),
            stats: Stats::default(),
        })
    }

    /// Identities whose socket could not be made.
    pub fn failed(&self) -> &[String] {
        &self.failed
    }

    /// The counters.
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Streams that are open (not counting those still sending their hello).
    pub fn streams(&self) -> usize {
        self.conns
            .iter()
            .filter(|c| matches!(c.phase, Phase::Open(_)))
            .count()
    }

    /// Bytes waiting for each recording client, by identity.
    pub fn recorder_backlogs(&self) -> Vec<(String, usize)> {
        self.conns
            .iter()
            .filter_map(|c| match &c.phase {
                Phase::Open(o) => o.rec.as_ref().map(|r| (c.identity.clone(), r.backlog())),
                _ => None,
            })
            .collect()
    }

    /// There is a card.
    pub fn has_card(&self) -> bool {
        self.device.is_some()
    }

    /// Nothing connected and nothing playing: the loop may sleep longer.
    pub fn idle(&self) -> bool {
        self.conns.is_empty() && self.mix.is_empty() && self.bus_users.is_empty()
    }

    /// What happened since the last call.
    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Both channels summed into each (the accessibility setting), for every stream at once.
    pub fn set_mono(&mut self, on: bool) {
        self.mono = on;
    }

    /// Who asked for the microphone over the bus (`audio.capture`); empty: nobody.
    pub fn set_bus_capture(&mut self, users: Vec<String>) {
        if users.is_empty() {
            self.bus_acc.clear();
        }
        self.bus_users = users;
    }

    /// Plays a clip of the bus (mono, `rate`) as a prompt; `ClipDone` follows.
    pub fn play_clip(&mut self, request: u64, rate: u32, samples: &[i16]) {
        let ok_rate = (ultar_audio_stream::MIN_RATE..=ultar_audio_stream::MAX_RATE).contains(&rate);
        if self.device.is_none() || !ok_rate || samples.is_empty() {
            self.events.push(Event::ClipDone { request, ok: false });
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut res = Resampler::new(rate, CARD_RATE);
        let mut out = Vec::new();
        res.process(&crate::resample::to_f32(samples), &mut out);
        let frames: Vec<(f32, f32)> = out.into_iter().map(|s| (s, s)).collect();
        self.mix.add(id, Class::Prompt, ultar_audio_stream::UNITY);
        let rest: VecDeque<(f32, f32)> = frames.into();
        self.clips.insert(id, Clip { request, rest });
    }

    /// Hands the mixer what of the clips it has room for; a clip that is all given ends.
    fn feed_clips(&mut self) {
        for (id, clip) in &mut self.clips {
            let n = self.mix.room(*id).min(clip.rest.len());
            let chunk: Vec<(f32, f32)> = clip.rest.drain(..n).collect();
            self.mix.push(*id, &chunk);
            if clip.rest.is_empty() {
                self.mix.end(*id);
            }
        }
    }

    /// One round: time passes, programs connect, sound is mixed and the microphone fanned
    /// out.
    pub fn step(&mut self, now: Instant) {
        let frames = self.advance(now);
        self.maintain_card(now);
        self.accept(now);
        self.pump(now);
        let far = self.play(frames);
        self.record(far);
        self.finish();
        self.publish_mic();
    }

    fn advance(&mut self, now: Instant) -> u64 {
        let Some(last) = self.last.replace(now) else {
            return 0;
        };
        let exact =
            now.saturating_duration_since(last).as_secs_f64() * f64::from(CARD_RATE) + self.frac;
        let frames = (exact.floor() as u64).min(MAX_STEP_FRAMES);
        self.frac = exact - exact.floor();
        frames
    }

    fn lose_card(&mut self, now: Instant) {
        self.device = None;
        self.stats.card_lost += 1;
        self.queued = 0;
        self.reference.clear();
        self.events.push(Event::Card(false));
        self.next_open = Some(now + REOPEN);
        // the streams cannot go on: clients see the end and run silent
        for c in self.conns.drain(..) {
            if c.voice {
                self.mix.remove(c.id);
            }
        }
        for (id, clip) in self.clips.drain() {
            self.mix.remove(id);
            self.events.push(Event::ClipDone {
                request: clip.request,
                ok: false,
            });
        }
    }

    fn maintain_card(&mut self, now: Instant) {
        if self.device.is_some() || self.next_open.is_some_and(|t| now < t) {
            return;
        }
        match (self.opener)() {
            Ok(d) => {
                self.device = Some(d);
                self.fresh = true;
                self.next_open = None;
                self.events.push(Event::Card(true));
            }
            Err(_) => self.next_open = Some(now + REOPEN),
        }
    }

    fn accept(&mut self, now: Instant) {
        for l in &self.listeners {
            while let Ok((sock, _)) = l.listener.accept() {
                // who is on the other end is what the kernel recorded at connect; a
                // kernel that does not say refuses everybody (fail closed)
                if !peer_credentials(&sock).is_ok_and(|c| c.uid == l.uid) {
                    self.stats.wrong_peer += 1;
                    continue;
                }
                if self.conns.len() >= MAX_CONNS || sock.set_nonblocking(true).is_err() {
                    continue;
                }
                let id = self.next_id;
                self.next_id += 1;
                self.conns
                    .push(Conn::new(id, l.identity.clone(), l.rights, sock, now));
            }
        }
    }

    fn status_for(&self, c: &Conn, h: &ultar_audio_stream::Hello) -> Status {
        let open = self
            .conns
            .iter()
            .filter(|o| matches!(o.phase, Phase::Open(_)))
            .count();
        if !c.rights.allow(h) {
            Status::Denied
        } else if self.device.is_none() {
            Status::NoDevice
        } else if open >= MAX_STREAMS {
            Status::TooMany
        } else {
            Status::Ok
        }
    }

    fn pump(&mut self, now: Instant) {
        self.feed_clips();
        for i in 0..self.conns.len() {
            if matches!(self.conns[i].phase, Phase::Hello(_)) {
                match self.conns[i].greet(now) {
                    Greeted::Waiting => {}
                    Greeted::Refused => self.conns[i].phase = Phase::Closing,
                    Greeted::Hello(h) => {
                        let status = self.status_for(&self.conns[i], &h);
                        let sent = self.conns[i].reply(status);
                        if status == Status::Ok && sent {
                            self.stats.opened += 1;
                            let c = &mut self.conns[i];
                            c.open(h);
                            if h.direction.plays() {
                                c.voice = true;
                                self.mix.add(c.id, h.class, h.volume);
                            }
                        } else {
                            if status == Status::Denied {
                                self.stats.denied += 1;
                            }
                            self.conns[i].phase = Phase::Closing;
                        }
                    }
                }
            }
            self.pump_open(i);
        }
    }

    fn pump_open(&mut self, i: usize) {
        let c = &mut self.conns[i];
        let id = c.id;
        let Phase::Open(open) = &mut c.phase else {
            return;
        };
        let mut dead = false;
        if let Some(play) = open.play.as_mut() {
            let room = self.mix.room(id);
            if room > 0 {
                match read_some(&mut c.sock, play.bytes_for(room)) {
                    Ok(Some(b)) => self.mix.push(id, &play.take(&b)),
                    Ok(None) => {}
                    Err(_) => dead = true,
                }
            }
        } else if read_some(&mut c.sock, 64).is_err() {
            // a recording client sends nothing; reading shows that it has gone
            dead = true;
        }
        if let Some(rec) = open.rec.as_mut() {
            dead |= rec.flush(&mut c.sock).is_err();
        }
        if dead {
            if c.voice {
                self.mix.end(id);
            }
            c.phase = Phase::Closing;
        }
    }

    /// Keeps the card's queue at [`QUEUE`]; returns the mono sound that left the queue (what
    /// the speaker played while this step's time passed).
    fn play(&mut self, frames: u64) -> Vec<f32> {
        self.queued -= frames.min(self.queued);
        let far: Vec<f32> = (0..frames)
            .map(|_| self.reference.pop_front().unwrap_or(0.0))
            .collect();
        let mut lost = false;
        if let Some(dev) = self.device.as_mut() {
            while self.queued < QUEUE && self.mix.sounding() {
                let mut buf = vec![0.0f32; PERIOD * 2];
                let done = self.mix.mix(&mut buf);
                self.finished.extend(done);
                if self.mono {
                    for f in buf.chunks_exact_mut(2) {
                        let m = (f[0] + f[1]) / 2.0;
                        f[0] = m;
                        f[1] = m;
                    }
                }
                let pcm = to_i16(&buf);
                match dev.write(&pcm) {
                    Ok(n) => {
                        let taken = n / 2;
                        self.queued += taken as u64;
                        self.reference
                            .extend(buf.chunks_exact(2).take(taken).map(|f| (f[0] + f[1]) / 2.0));
                        if n < pcm.len() {
                            // the card is full: the rest is dropped, not piled up
                            self.queued = self.queued.max(FULL);
                            break;
                        }
                    }
                    Err(_) => {
                        lost = true;
                        break;
                    }
                }
            }
        }
        if lost {
            self.lose_card(self.last.unwrap_or_else(Instant::now));
        }
        far
    }

    fn recorders(&self) -> impl Iterator<Item = &Conn> {
        self.conns.iter().filter(|c| c.records())
    }

    fn record(&mut self, far: Vec<f32>) {
        let any = self.recorders().next().is_some() || !self.bus_users.is_empty();
        if !any {
            self.fresh = true;
            return;
        }
        let mut lost = false;
        let mut mic48 = Vec::new();
        if let Some(dev) = self.device.as_mut() {
            let mut buf = vec![0i16; 9600];
            if self.fresh {
                // what piled up while nobody listened is old: it is thrown away
                for _ in 0..64 {
                    if !matches!(dev.read(&mut buf), Ok(n) if n > 0) {
                        break;
                    }
                }
                self.fresh = false;
            }
            for _ in 0..4 {
                match dev.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => mic48.extend(mono_of(&buf[..n])),
                    Err(_) => {
                        lost = true;
                        break;
                    }
                }
            }
        }
        if lost {
            self.lose_card(self.last.unwrap_or_else(Instant::now));
            return;
        }
        let echo = self.recorders().any(|c| {
            matches!(&c.phase, Phase::Open(o) if o.rec.as_ref().is_some_and(|r| r.wants_echo_path()))
        });
        let (mut m16, mut f16) = (Vec::new(), Vec::new());
        if echo || !self.bus_users.is_empty() {
            self.mic16.process(&mic48, &mut m16);
        }
        if echo {
            self.far16.process(&far, &mut f16);
        }
        for c in &mut self.conns {
            if let Phase::Open(o) = &mut c.phase {
                if let Some(rec) = o.rec.as_mut() {
                    rec.feed(&mic48, &m16, &f16);
                }
            }
        }
        if !self.bus_users.is_empty() {
            self.bus_acc.extend(to_i16(&m16));
            while self.bus_acc.len() >= CHUNK {
                let chunk: Vec<i16> = self.bus_acc.drain(..CHUNK).collect();
                self.events.push(Event::Chunk(chunk));
            }
        }
    }

    fn finish(&mut self) {
        let mut gone = std::mem::take(&mut self.finished);
        gone.extend(self.mix.reap());
        gone.sort_unstable();
        for id in &gone {
            if let Some(clip) = self.clips.remove(id) {
                self.events.push(Event::ClipDone {
                    request: clip.request,
                    ok: true,
                });
            }
        }
        self.conns.retain(|c| {
            !(matches!(c.phase, Phase::Closing) && (!c.voice || gone.binary_search(&c.id).is_ok()))
        });
    }

    fn publish_mic(&mut self) {
        let mut users: Vec<String> = self
            .recorders()
            .map(|c| c.identity.clone())
            .chain(self.bus_users.iter().cloned())
            .collect();
        users.sort();
        users.dedup();
        if users != self.mic_now {
            self.mic_now.clone_from(&users);
            self.events.push(Event::Mic(users));
        }
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
