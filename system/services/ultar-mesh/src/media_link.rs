//! "Play there": the sound of a media session goes to a paired device with a speaker and
//! pause, resume and volume work from both ends. Gated like calls by [`Right::Call`]
//! ("Anrufe und Ton", off after pairing) on the receiving side, the sound is the same
//! 20 ms PCM frames as a call's voice ([`crate::audio_link`]), all of it end to end
//! encrypted by the channel.

use crate::audio_link;
use crate::device;
use crate::node::{Effect, Node};
use crate::wire::Frame;
use std::collections::{BTreeMap, BTreeSet};
use ultar_bus::messages::mesh::{Capability, Right};
use ultar_bus::messages::mesh_media::{
    MediaControl, MediaCtl, MediaIncoming, MediaPcmIn, MediaPhase, MediaProgress, MAX_TEXT,
};

/// Most "play there" sessions a device has open at once (as source plus as sink).
pub const MAX_SESSIONS: usize = 4;

/// The sessions of a node.
#[derive(Default)]
pub(crate) struct Media {
    next: u64,
    /// Sessions this device sends (id, device that plays).
    out: BTreeMap<u64, String>,
    /// Sessions that play here (device, its id).
    sinks: BTreeSet<(String, u64)>,
}

impl Media {
    /// Ids start at a number of the device's own so that two devices never meet.
    pub(crate) fn salted(device_id: &str) -> Media {
        let salt = device_id.bytes().fold(0x811c_9dc5_u32, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
        });
        Media {
            next: u64::from(salt) << 32,
            ..Media::default()
        }
    }

    fn open_count(&self) -> usize {
        self.out.len() + self.sinks.len()
    }

    /// Ends every session with `peer` (the link is gone).
    pub(crate) fn peer_down(&mut self, peer: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        let out: Vec<u64> = self
            .out
            .iter()
            .filter(|(_, p)| p.as_str() == peer)
            .map(|(id, _)| *id)
            .collect();
        for id in out {
            self.out.remove(&id);
            effects.push(ended(id, peer));
        }
        let sinks: Vec<(String, u64)> = self
            .sinks
            .iter()
            .filter(|(p, _)| p == peer)
            .cloned()
            .collect();
        for key in sinks {
            self.sinks.remove(&key);
            effects.push(ended(key.1, peer));
        }
        effects
    }
}

fn ended(id: u64, device: &str) -> Effect {
    Effect::MediaProgress(MediaProgress {
        id,
        device: device.to_owned(),
        phase: MediaPhase::Ended,
    })
}

/// `text` without control characters (a title may be empty).
fn plain(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

fn cut(mut text: String) -> String {
    let mut end = MAX_TEXT.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

impl Node {
    /// Starts a session that plays on the paired device `device`; `0` when it cannot (not
    /// paired, not reachable, no speaker, too many sessions).
    pub fn media_open(&mut self, device: &str, title: &str, artist: &str) -> (u64, Vec<Effect>) {
        let speaker = self.links.get(device).is_some_and(|l| {
            self.paired(device)
                && l.device
                    .as_ref()
                    .is_some_and(|d| d.can(Capability::AudioOut))
        });
        if !speaker || self.media.open_count() >= MAX_SESSIONS {
            return (0, Vec::new());
        }
        self.media.next += 1;
        let id = self.media.next;
        self.media.out.insert(id, device.to_owned());
        let frame = Frame::MediaOpen {
            id,
            title: cut(plain(title)),
            artist: cut(plain(artist)),
        };
        (id, vec![Effect::Send(device.to_owned(), frame)])
    }

    /// Sends one frame of the session `id` to `device`; refused for a wrong size or an
    /// unknown session.
    pub fn media_pcm(&mut self, device: &str, id: u64, seq: u64, pcm: &[u8]) -> Vec<Effect> {
        let known = self.media.out.get(&id).is_some_and(|p| p == device);
        if !known || !self.paired(device) || !audio_link::is_frame(pcm) {
            return Vec::new();
        }
        vec![Effect::Send(
            device.to_owned(),
            Frame::MediaPcm {
                id,
                seq,
                pcm: device::encode(pcm),
            },
        )]
    }

    /// A control toward the other end of session `id` (`device` plays, or is the source).
    /// [`MediaCtl::Stop`] ends the session here too.
    pub fn media_ctl(&mut self, device: &str, id: u64, ctl: MediaCtl) -> Vec<Effect> {
        let source = self.media.out.get(&id).is_some_and(|p| p == device);
        let sink = self.media.sinks.contains(&(device.to_owned(), id));
        if !(source || sink) || !self.paired(device) {
            return Vec::new();
        }
        let mut effects = vec![Effect::Send(device.to_owned(), Frame::MediaCtl { id, ctl })];
        if ctl == MediaCtl::Stop {
            self.media.out.remove(&id);
            self.media.sinks.remove(&(device.to_owned(), id));
            effects.push(ended(id, device));
        }
        effects
    }

    pub(crate) fn on_media_frame(&mut self, peer: &str, frame: Frame) -> Vec<Effect> {
        if !self.allows(peer, Right::Call) {
            return Vec::new();
        }
        match frame {
            Frame::MediaOpen { id, title, artist } => self.on_media_open(peer, id, title, artist),
            Frame::MediaPcm { id, seq, pcm } => self.on_media_pcm(peer, id, seq, pcm),
            Frame::MediaCtl { id, ctl } => self.on_media_ctl(peer, id, ctl),
            _ => Vec::new(),
        }
    }

    fn on_media_open(&mut self, peer: &str, id: u64, title: String, artist: String) -> Vec<Effect> {
        let can = self.config.capabilities.contains(&Capability::AudioOut);
        let key = (peer.to_owned(), id);
        let fits = title.len() <= MAX_TEXT && artist.len() <= MAX_TEXT;
        if !can
            || !fits
            || self.media.sinks.contains(&key)
            || self.media.open_count() >= MAX_SESSIONS
        {
            return Vec::new();
        }
        self.media.sinks.insert(key);
        let from_name = self
            .links
            .get(peer)
            .and_then(|l| l.device.as_ref())
            .map_or_else(|| "device".to_owned(), |d| d.name.clone());
        vec![Effect::MediaIncoming(MediaIncoming {
            id,
            from: peer.to_owned(),
            from_name,
            title,
            artist,
        })]
    }

    fn on_media_pcm(&mut self, peer: &str, id: u64, seq: u64, pcm: String) -> Vec<Effect> {
        let known = self.media.sinks.contains(&(peer.to_owned(), id));
        let sized = device::decode(&pcm).is_some_and(|b| audio_link::is_frame(&b));
        if !known || !sized {
            return Vec::new();
        }
        vec![Effect::MediaPcm(MediaPcmIn {
            id,
            from: peer.to_owned(),
            seq,
            pcm,
        })]
    }

    fn on_media_ctl(&mut self, peer: &str, id: u64, ctl: MediaCtl) -> Vec<Effect> {
        let source = self.media.out.get(&id).is_some_and(|p| p == peer);
        let sink = self.media.sinks.contains(&(peer.to_owned(), id));
        if !(source || sink) {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if ctl == MediaCtl::Stop {
            self.media.out.remove(&id);
            self.media.sinks.remove(&(peer.to_owned(), id));
            effects.push(ended(id, peer));
        }
        effects.push(Effect::MediaControl(MediaControl {
            id,
            from: peer.to_owned(),
            ctl,
        }));
        effects
    }
}
