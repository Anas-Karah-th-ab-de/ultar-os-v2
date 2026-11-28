//! The bus side of "play there" (mesh item C3).
//!
//! As the **source** the media service offers the paired devices with a speaker as outputs
//! (`media.there`), asks the mesh to open a session on the one the person picks
//! (`media.set_output`), sends the 20 ms frames the player hands over (`media.there_push`)
//! and passes pause, resume and volume both ways. As the **sink** it plays what arrives on
//! the speaker ([`Sink`]) at a 20 ms beat that runs only while a session is open.
//!
//! Nothing here touches a server: the bus carries everything to the local mesh service,
//! which sends it end to end encrypted to the other device.

use crate::service::MediaService;
use crate::there::{self, Playback, Sink};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::media::{Button, Command};
use ultar_bus::messages::mesh::{Done, MeshState};
use ultar_bus::messages::mesh_media::{
    MediaControl, MediaCtl, MediaCtlOut, MediaIncoming, MediaOpen, MediaPcmIn, MediaPcmOut,
    MediaProgress, SetThere, ThereOutput, ThereReply, TherePause, TherePush, ThereState,
    ThereVolume,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_svckit::Bus;

/// How long the mesh service may take to take one request.
const MESH_WAIT: Duration = Duration::from_millis(200);
/// The beat of the speaker.
const BEAT: Duration = Duration::from_millis(20);

/// The session that sends the sound elsewhere.
struct Active {
    device: String,
    id: u64,
    /// Last frame counter sent (the player's counter only goes up).
    last_seq: Option<u64>,
}

/// "Play there" state of the media service.
#[derive(Default)]
pub struct ThereRt {
    sink: Option<Box<dyn Sink>>,
    outputs: Vec<ThereOutput>,
    active: Option<Active>,
    paused: bool,
    volume: u8,
    playback: Option<Playback>,
    run: Arc<AtomicBool>,
    wake: Option<Sender<()>>,
    published: Option<ThereState>,
}

impl ThereRt {
    /// A state whose speaker is `sink` (`None`: this device cannot play for others).
    pub fn new(sink: Option<Box<dyn Sink>>) -> Self {
        Self {
            sink,
            volume: 100,
            ..Self::default()
        }
    }

    fn state(&self) -> ThereState {
        ThereState {
            outputs: self.outputs.clone(),
            active: self.active.as_ref().map(|a| a.device.clone()),
            paused: self.paused,
            volume: self.volume,
            playing_for: self.playback.as_ref().map(|p| p.name.clone()),
        }
    }
}

fn mesh<C: Call>(bus: &Bus<'_, MediaService>, call: &C) -> Option<C::Reply> {
    bus.call(call, MESH_WAIT).ok()
}

impl MediaService {
    /// After start: the pacer of the speaker and the first state.
    pub(crate) fn there_start(&mut self, bus: &Bus<'_, Self>) {
        if self.there.sink.is_some() {
            let (wake, asleep) = channel::<()>();
            let run = self.there.run.clone();
            let poster = bus.poster();
            let spawned = std::thread::Builder::new()
                .name("there-pacer".into())
                .spawn(move || {
                    // Blocks (no wake-ups at all) until a session opens.
                    while asleep.recv().is_ok() {
                        while run.load(Ordering::SeqCst) {
                            if !poster.post(Box::new(|s: &mut Self, b: &Bus<'_, Self>| s.pump(b))) {
                                return;
                            }
                            std::thread::sleep(BEAT);
                        }
                    }
                });
            if spawned.is_ok() {
                self.there.wake = Some(wake);
            }
        }
        self.there_publish(bus);
    }

    fn there_publish(&mut self, bus: &Bus<'_, Self>) {
        let state = self.there.state();
        if self.there.published.as_ref() != Some(&state) {
            bus.retain(&state);
            self.there.published = Some(state);
        }
    }

    /// Plays the next 20 ms of the session that plays here.
    fn pump(&mut self, _bus: &Bus<'_, Self>) {
        let Some(playback) = self.there.playback.as_mut() else {
            self.there.run.store(false, Ordering::SeqCst);
            return;
        };
        let frame = playback.take();
        if let Some(sink) = self.there.sink.as_mut() {
            sink.play(&frame);
        }
    }

    fn end_playback(&mut self) {
        self.there.playback = None;
        self.there.run.store(false, Ordering::SeqCst);
        if let Some(sink) = self.there.sink.as_mut() {
            sink.close();
        }
    }

    /// Tells the player to pause or play when it is not doing that yet.
    fn player_to(&self, paused: bool, bus: &Bus<'_, Self>) {
        let state = self.session.state();
        if let Some(now) = state.now.filter(|n| n.playing == paused) {
            bus.publish(&Command {
                player: now.player,
                button: Button::PlayPause,
            });
        }
    }

    fn stop_active(&mut self, bus: &Bus<'_, Self>) {
        if let Some(a) = self.there.active.take() {
            let _ = mesh(
                bus,
                &MediaCtlOut {
                    device: a.device,
                    id: a.id,
                    ctl: MediaCtl::Stop,
                },
            );
        }
        self.there.paused = false;
    }

    /// `media.set_output`, `media.there_volume`, `media.there_pause`, `media.there_push`.
    pub(crate) fn there_request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let method = request.method().to_owned();
        let player = self.session.state().now.map(|n| n.player);
        let system = self.access.is_system(request.caller());
        let ok = if method == TherePush::METHOD {
            let holder = player.as_deref() == Some(request.caller());
            request
                .decode::<TherePush>()
                .map(|c| holder && self.push(&c, bus))
        } else if !system {
            let _ = request.fail(ErrorCode::PermissionDenied);
            return;
        } else if method == SetThere::METHOD {
            request.decode::<SetThere>().map(|c| self.set(c, bus))
        } else if method == ThereVolume::METHOD {
            request
                .decode::<ThereVolume>()
                .map(|c| self.volume(c.percent.min(100), bus))
        } else {
            request.decode::<TherePause>().map(|c| self.pause(c.on, bus))
        };
        match ok {
            Ok(ok) => {
                let _ = request.reply(&ThereReply { ok });
                self.there_publish(bus);
            }
            Err(_) => {
                let _ = request.fail(ErrorCode::BadPayload);
            }
        }
    }

    fn push(&mut self, c: &TherePush, bus: &Bus<'_, Self>) -> bool {
        let paused = self.there.paused;
        let Some(a) = self.there.active.as_mut().filter(|_| !paused) else {
            return false;
        };
        if a.last_seq.is_some_and(|l| c.seq <= l) || there::decode(&c.pcm).is_none() {
            return false;
        }
        a.last_seq = Some(c.seq);
        let frame = MediaPcmOut {
            device: a.device.clone(),
            id: a.id,
            seq: c.seq,
            pcm: c.pcm.clone(),
        };
        mesh(bus, &frame).is_some_and(|d: Done| d.ok)
    }

    fn set(&mut self, c: SetThere, bus: &Bus<'_, Self>) -> bool {
        let Some(device) = c.device else {
            self.stop_active(bus);
            return true;
        };
        if !self.there.outputs.iter().any(|o| o.device == device) {
            return false;
        }
        if self.there.active.as_ref().is_some_and(|a| a.device == device) {
            return true;
        }
        self.stop_active(bus);
        let (title, artist) = self
            .session
            .state()
            .now
            .map(|n| (n.title, n.artist))
            .unwrap_or_default();
        let open = MediaOpen {
            device: device.clone(),
            title,
            artist,
        };
        match mesh(bus, &open).filter(|o| o.id != 0) {
            Some(o) => {
                self.there.active = Some(Active {
                    device,
                    id: o.id,
                    last_seq: None,
                });
                self.there.volume = 100;
                true
            }
            None => false,
        }
    }

    fn volume(&mut self, percent: u8, bus: &Bus<'_, Self>) -> bool {
        if let Some(a) = &self.there.active {
            self.there.volume = percent;
            let ctl = MediaCtlOut {
                device: a.device.clone(),
                id: a.id,
                ctl: MediaCtl::Volume(percent),
            };
            return mesh(bus, &ctl).is_some_and(|d: Done| d.ok);
        }
        let Some(p) = self.there.playback.as_mut() else {
            return false;
        };
        p.volume = percent;
        let ctl = MediaCtlOut {
            device: p.from.clone(),
            id: p.id,
            ctl: MediaCtl::Volume(percent),
        };
        mesh(bus, &ctl).is_some_and(|d: Done| d.ok)
    }

    fn pause(&mut self, on: bool, bus: &Bus<'_, Self>) -> bool {
        let ctl = if on { MediaCtl::Pause } else { MediaCtl::Resume };
        if let Some(a) = &self.there.active {
            self.there.paused = on;
            let out = MediaCtlOut {
                device: a.device.clone(),
                id: a.id,
                ctl,
            };
            self.player_to(on, bus);
            return mesh(bus, &out).is_some_and(|d: Done| d.ok);
        }
        let Some(p) = self.there.playback.as_mut() else {
            return false;
        };
        p.paused = on;
        let out = MediaCtlOut {
            device: p.from.clone(),
            id: p.id,
            ctl,
        };
        mesh(bus, &out).is_some_and(|d: Done| d.ok)
    }

    /// The mesh reported: devices, a session, sound, a control or an end.
    pub(crate) fn there_event(&mut self, event: &RawEvent, bus: &Bus<'_, Self>) {
        if event.topic == MeshState::TOPIC {
            if let Ok(state) = event.decode::<MeshState>() {
                self.there.outputs = there::outputs(&state);
                let gone = self.there.active.as_ref().is_some_and(|a| {
                    !self.there.outputs.iter().any(|o| o.device == a.device)
                });
                if gone {
                    self.there.active = None;
                    self.there.paused = false;
                }
            }
        } else if event.topic == MediaIncoming::TOPIC {
            if let Ok(c) = event.decode::<MediaIncoming>() {
                self.incoming(c, bus);
            }
        } else if event.topic == MediaPcmIn::TOPIC {
            if let Ok(c) = event.decode::<MediaPcmIn>() {
                if let (Some(p), Some(frame)) =
                    (self.there.playback.as_mut(), there::decode(&c.pcm))
                {
                    if p.id == c.id && p.from == c.from {
                        p.put(c.seq, frame);
                    }
                }
            }
        } else if event.topic == MediaControl::TOPIC {
            if let Ok(c) = event.decode::<MediaControl>() {
                self.control(&c, bus);
            }
        } else if event.topic == MediaProgress::TOPIC {
            if let Ok(c) = event.decode::<MediaProgress>() {
                let source = self.there.active.as_ref().is_some_and(|a| a.id == c.id);
                if source {
                    self.there.active = None;
                    self.there.paused = false;
                }
                if self.there.playback.as_ref().is_some_and(|p| p.id == c.id) {
                    self.end_playback();
                }
            }
        }
        self.there_publish(bus);
    }

    /// A device wants to play its sound here: taken when this device has a free speaker.
    fn incoming(&mut self, c: MediaIncoming, bus: &Bus<'_, Self>) {
        let free = self.there.playback.is_none();
        let taken = free && self.there.sink.as_mut().is_some_and(|s| s.open());
        if !taken {
            let stop = MediaCtlOut {
                device: c.from,
                id: c.id,
                ctl: MediaCtl::Stop,
            };
            let _ = mesh(bus, &stop);
            return;
        }
        self.there.playback = Some(Playback::new(c.id, c.from, c.from_name));
        self.there.run.store(true, Ordering::SeqCst);
        if let Some(wake) = &self.there.wake {
            let _ = wake.send(());
        }
    }

    /// The other end pressed pause, resume, volume or stop.
    fn control(&mut self, c: &MediaControl, bus: &Bus<'_, Self>) {
        let source = self
            .there
            .active
            .as_ref()
            .is_some_and(|a| a.id == c.id && a.device == c.from);
        if source {
            match c.ctl {
                MediaCtl::Pause | MediaCtl::Resume => {
                    let on = c.ctl == MediaCtl::Pause;
                    self.there.paused = on;
                    self.player_to(on, bus);
                }
                MediaCtl::Volume(p) => self.there.volume = p.min(100),
                MediaCtl::Stop => {
                    self.there.active = None;
                    self.there.paused = false;
                }
            }
            return;
        }
        let sink = self
            .there
            .playback
            .as_ref()
            .is_some_and(|p| p.id == c.id && p.from == c.from);
        if !sink {
            return;
        }
        match (c.ctl, self.there.playback.as_mut()) {
            (MediaCtl::Stop, _) => self.end_playback(),
            (MediaCtl::Pause, Some(p)) => p.paused = true,
            (MediaCtl::Resume, Some(p)) => p.paused = false,
            (MediaCtl::Volume(v), Some(p)) => p.volume = v.min(100),
            _ => {}
        }
    }
}
