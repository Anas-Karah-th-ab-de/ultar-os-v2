//! The running call: finds a way between the two devices (directly in one network or through
//! the internet, else through the TURN relay), then carries sealed voice every 20 ms and, in a
//! video call, sealed pictures. Microphone, speaker, camera and screen are ports ([`AudioIo`],
//! [`VideoIo`]); everything that leaves the device is sealed with the call's media keys.

use crate::e2e::audio::{self, Jitter, FRAME};
use crate::e2e::crypto::MediaKeys;
use crate::e2e::net::{Net, Route};
use crate::e2e::video::{self, Assembler, Picture};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PROBE: u8 = 1;
const ACK: u8 = 2;
const VOICE: u8 = 3;
const PICTURE: u8 = 4;
const ALIVE: u8 = 5;

/// How long a way may be searched.
const SEARCH: Duration = Duration::from_secs(12);
/// Time a better way gets after the first one answered.
const BETTER: Duration = Duration::from_millis(400);
/// Silence from the other side after which the call is lost.
const LOST: Duration = Duration::from_secs(20);
/// Pictures per second.
const FPS: u64 = 12;

/// Microphone and speaker (16 kHz mono, 20 ms frames of [`FRAME`] samples).
pub trait AudioIo: Send {
    /// The next 20 ms of the microphone, if ready (never blocks).
    fn capture(&mut self) -> Option<Vec<i16>>;
    /// Plays 20 ms.
    fn play(&mut self, frame: &[i16]);
}

/// Camera and the window that shows the other side.
pub trait VideoIo: Send {
    /// The next picture of the camera, if one is ready (never blocks).
    fn capture(&mut self) -> Option<Picture>;
    /// Shows a picture of the other side.
    fn show(&mut self, picture: Picture);
}

/// What the media part reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaEvent {
    /// A way was found: directly or through the relay.
    Connected {
        /// Through the TURN relay.
        relayed: bool,
    },
    /// No way to the other device, or it went silent.
    Lost,
}

/// One running call's media.
pub struct Media {
    net: Net,
    keys: MediaKeys,
    peer: Vec<SocketAddr>,
    route: Option<Route>,
    heard: Instant,
    seq: u64,
    jitter: Jitter,
    assembler: Assembler,
    picture: u32,
    muted: Arc<AtomicBool>,
}

impl Media {
    /// Media over `net` towards the peer's addresses.
    pub fn new(
        mut net: Net,
        keys: MediaKeys,
        peer: Vec<SocketAddr>,
        muted: Arc<AtomicBool>,
    ) -> Self {
        net.permit(&peer);
        Self {
            net,
            keys,
            peer,
            route: None,
            heard: Instant::now(),
            seq: 0,
            jitter: Jitter::default(),
            assembler: Assembler::default(),
            picture: 0,
            muted,
        }
    }

    fn routes(&self) -> Vec<Route> {
        let mut r: Vec<Route> = if self.net.relay_only {
            Vec::new()
        } else {
            self.peer.iter().map(|p| Route::Direct(*p)).collect()
        };
        if self.net.has_relay() {
            r.extend(self.peer.iter().map(|p| Route::Relayed(*p)));
        }
        r
    }

    fn send(&mut self, route: Route, kind: u8, payload: &[u8]) {
        let mut packet = vec![kind];
        packet.extend(self.keys.out.seal(&[kind], payload));
        let _ = self.net.send(route, &packet);
    }

    /// Opens a packet: kind and payload.
    fn open(&mut self, packet: &[u8]) -> Option<(u8, Vec<u8>)> {
        let (kind, sealed) = packet.split_first()?;
        let plain = self.keys.inn.open(&[*kind], sealed).ok()?;
        Some((*kind, plain))
    }

    /// Searches a way; true when one answered.
    pub fn find_way(&mut self, stop: &AtomicBool) -> bool {
        let start = Instant::now();
        let mut next_probe = start;
        let mut found: Option<(Route, Instant)> = None;
        while start.elapsed() < SEARCH && !stop.load(Ordering::Relaxed) {
            if Instant::now() >= next_probe {
                for r in self.routes() {
                    self.send(r, PROBE, b"probe");
                }
                next_probe = Instant::now() + Duration::from_millis(150);
            }
            if let Some((route, bytes)) = self.net.recv(Duration::from_millis(20)) {
                match self.open(&bytes) {
                    Some((PROBE, _)) => self.send(route, ACK, b"ack"),
                    Some((ACK | VOICE | PICTURE | ALIVE, _)) => {
                        let better = found.is_none_or(|(f, _)| route.rank() < f.rank());
                        if better {
                            found = Some((route, found.map_or(Instant::now(), |(_, t)| t)));
                        }
                    }
                    _ => {}
                }
            }
            if let Some((route, t)) = found {
                if route.rank() == 0 || t.elapsed() >= BETTER {
                    self.route = Some(route);
                    self.heard = Instant::now();
                    return true;
                }
            }
        }
        false
    }

    /// True when the way goes through the relay.
    pub fn relayed(&self) -> bool {
        matches!(self.route, Some(Route::Relayed(_)))
    }

    /// Carries the call until `stop` or the other side goes silent.
    pub fn run(
        &mut self,
        audio: &mut dyn AudioIo,
        mut video: Option<&mut dyn VideoIo>,
        stop: &AtomicBool,
        events: &Sender<MediaEvent>,
    ) {
        let tick = Duration::from_millis(20);
        let mut next_voice = Instant::now();
        let mut next_picture = Instant::now();
        let mut next_keep = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            if let Some((route, bytes)) = self.net.recv(Duration::from_millis(4)) {
                self.receive(route, &bytes, &mut video);
            }
            let now = Instant::now();
            if now >= next_voice {
                next_voice += tick;
                self.voice(audio);
            }
            if now >= next_picture {
                next_picture = now + Duration::from_millis(1000 / FPS);
                if let Some(v) = video.as_mut() {
                    self.picture(&mut **v);
                }
            }
            if now >= next_keep {
                next_keep = now + Duration::from_secs(1);
                self.net.keep();
                if let Some(r) = self.route {
                    self.send(r, ALIVE, b"");
                }
            }
            if self.heard.elapsed() > LOST {
                let _ = events.send(MediaEvent::Lost);
                return;
            }
        }
    }

    fn receive(&mut self, route: Route, bytes: &[u8], video: &mut Option<&mut dyn VideoIo>) {
        if self.net.relay_only && matches!(route, Route::Direct(_)) {
            return;
        }
        let Some((kind, payload)) = self.open(bytes) else {
            return; // not sealed with this call's key: ignored
        };
        self.heard = Instant::now();
        // the other side moved (for example from the relay to a direct way): follow it
        if kind != PROBE && self.route.is_none_or(|r| route.rank() <= r.rank()) {
            self.route = Some(route);
        }
        match kind {
            PROBE => self.send(route, ACK, b"ack"),
            VOICE if payload.len() > 8 => {
                let seq = u64::from_be_bytes(payload[..8].try_into().unwrap_or_default());
                if let Some(frame) = audio::decode(&payload[8..]) {
                    self.jitter.put(seq, frame);
                }
            }
            PICTURE => {
                if let (Some(jpeg), Some(v)) = (self.assembler.put(&payload), video.as_mut()) {
                    if let Some(p) = video::decode(&jpeg) {
                        v.show(p);
                    }
                }
            }
            _ => {}
        }
    }

    fn voice(&mut self, audio: &mut dyn AudioIo) {
        audio.play(&self.jitter.take());
        let Some(frame) = audio.capture() else { return };
        let Some(route) = self.route else { return };
        let frame = if self.muted.load(Ordering::Relaxed) {
            vec![0; FRAME]
        } else {
            frame
        };
        let mut payload = self.seq.to_be_bytes().to_vec();
        payload.extend(audio::encode(&frame));
        self.seq += 1;
        self.send(route, VOICE, &payload);
    }

    fn picture(&mut self, v: &mut dyn VideoIo) {
        let (Some(route), Some(p)) = (self.route, v.capture()) else {
            return;
        };
        let Some(jpeg) = video::encode(&p) else {
            return;
        };
        self.picture += 1;
        for piece in video::pieces(self.picture, &jpeg) {
            self.send(route, PICTURE, &piece);
        }
    }

    /// Ends the call's media (gives the relay back).
    pub fn close(mut self) {
        self.net.close();
    }
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;
