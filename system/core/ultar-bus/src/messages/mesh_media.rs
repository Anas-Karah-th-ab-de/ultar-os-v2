//! Contract of "play there" (mesh item C3): the sound of a media session goes to another
//! paired device that has a speaker, and pause, resume and volume work from both sides.
//!
//! Two services use it. The mesh carries the sound as 20 ms frames of PCM 16-bit, 48 kHz
//! mono over the encrypted channel (`mesh.media_*`, gated by `Right::Call`, "Anrufe und
//! Ton", off after pairing). The media service (`media.there_*`) offers the paired devices
//! as outputs, takes the player's frames and sends them; on the other device it plays what
//! arrives. Nothing goes through a server.

use crate::message::{Call, Message, Topic};
use crate::messages::mesh::{Done, SERVICE as MESH};
use serde::{Deserialize, Serialize};

/// Longest title or artist that goes over the mesh.
pub const MAX_TEXT: usize = 128;
/// Most outputs the media service offers.
pub const MAX_OUTPUTS: usize = 8;

/// A control that goes between the two ends of a "play there" session.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaCtl {
    /// Stop playing for now (the sink goes silent, the source's player pauses).
    Pause,
    /// Play again.
    Resume,
    /// Volume of the sink, 0-100.
    Volume(u8),
    /// The session is over.
    Stop,
}

/// `mesh.media_open` - start a "play there" session on a paired device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct MediaOpen {
    /// The device that plays.
    pub device: String,
    /// What plays (data; shown on the other device).
    pub title: String,
    /// By whom (data).
    pub artist: String,
}

/// Answer to [`MediaOpen`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct MediaOpened {
    /// Id of the session on this device (`0`: the device cannot play sound or is not paired).
    pub id: u64,
}

/// `mesh.media_pcm` - one 20 ms frame to the playing device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct MediaPcmOut {
    /// The device that plays.
    pub device: String,
    /// The session.
    pub id: u64,
    /// Frame counter.
    pub seq: u64,
    /// PCM 16-bit, 48 kHz mono, base64 (one frame, see `ultar_mesh::audio_link`).
    pub pcm: String,
}

/// `mesh.media_ctl` - pause, resume, volume or stop, toward the other end.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct MediaCtlOut {
    /// The other end of the session.
    pub device: String,
    /// The session.
    pub id: u64,
    /// What to do.
    pub ctl: MediaCtl,
}

/// `mesh.media_incoming` - a paired device wants to play sound here (topic, not retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct MediaIncoming {
    /// The session on the other device's numbering.
    pub id: u64,
    /// The device that plays its sound here.
    pub from: String,
    /// Its name (data).
    pub from_name: String,
    /// What plays (data).
    pub title: String,
    /// By whom (data).
    pub artist: String,
}

/// `mesh.media_pcm_in` - one frame arrived from the source (topic, not retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct MediaPcmIn {
    /// The session.
    pub id: u64,
    /// The device it came from.
    pub from: String,
    /// Frame counter.
    pub seq: u64,
    /// PCM, base64.
    pub pcm: String,
}

/// `mesh.media_control` - the other end pressed a control (topic, not retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct MediaControl {
    /// The session.
    pub id: u64,
    /// The device it came from.
    pub from: String,
    /// What it asks.
    pub ctl: MediaCtl,
}

/// Where a "play there" session stands.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaPhase {
    /// The other end stopped it or the link is gone.
    Ended,
}

/// `mesh.media_progress` - a session ended (topic, not retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct MediaProgress {
    /// The session.
    pub id: u64,
    /// The other device.
    pub device: String,
    /// Where it stands.
    pub phase: MediaPhase,
}

/// One device the sound can go to.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ThereOutput {
    /// Its mesh id.
    pub device: String,
    /// Its name (data).
    pub name: String,
}

/// `media.there` - the "play there" state of this device (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ThereState {
    /// Paired, reachable devices with a speaker and the right "Anrufe und Ton" for the
    /// source (the sink decides on its side).
    pub outputs: Vec<ThereOutput>,
    /// The device the sound goes to now (`None`: this device plays it).
    pub active: Option<String>,
    /// Paused.
    pub paused: bool,
    /// Volume of the other device, 0-100.
    pub volume: u8,
    /// A device plays its sound here: its name (the sink side).
    pub playing_for: Option<String>,
}

/// `media.set_output` - send the sound to a device, or back here (`None`) (the shell).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetThere {
    /// The device from `ThereState::outputs`, or `None` for this device.
    pub device: Option<String>,
}

/// `media.there_volume` - volume at the other end (both sides: the shell here).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThereVolume {
    /// 0-100.
    pub percent: u8,
}

/// `media.there_pause` - pause or resume the session, from the source or the sink
/// (the shell).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct TherePause {
    /// Paused.
    pub on: bool,
}

/// `media.there_push` - the player gives the next 20 ms of what it plays while the sound
/// goes elsewhere (only the player that holds the session).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct TherePush {
    /// Frame counter.
    pub seq: u64,
    /// PCM 16-bit, 48 kHz mono, base64.
    pub pcm: String,
}

/// Answer to the `media.there_*` calls.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThereReply {
    /// Done.
    pub ok: bool,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    MediaCtl,
    MediaOpen,
    MediaOpened,
    MediaPcmOut,
    MediaCtlOut,
    MediaIncoming,
    MediaPcmIn,
    MediaControl,
    MediaPhase,
    MediaProgress,
    ThereOutput,
    ThereState,
    SetThere,
    ThereVolume,
    TherePause,
    TherePush,
    ThereReply
);

macro_rules! mesh_call {
    ($t:ty, $m:literal, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = MESH;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
mesh_call!(MediaOpen, "media_open", MediaOpened);
mesh_call!(MediaPcmOut, "media_pcm", Done);
mesh_call!(MediaCtlOut, "media_ctl", Done);

macro_rules! media_call {
    ($t:ty, $m:literal) => {
        impl Call for $t {
            const SERVICE: &'static str = crate::messages::media::SERVICE;
            const METHOD: &'static str = $m;
            type Reply = ThereReply;
        }
    };
}
media_call!(SetThere, "set_output");
media_call!(ThereVolume, "there_volume");
media_call!(TherePause, "there_pause");
media_call!(TherePush, "there_push");

impl Topic for MediaIncoming {
    const TOPIC: &'static str = "mesh.media_incoming";
}

impl Topic for MediaPcmIn {
    const TOPIC: &'static str = "mesh.media_pcm_in";
}

impl Topic for MediaControl {
    const TOPIC: &'static str = "mesh.media_control";
}

impl Topic for MediaProgress {
    const TOPIC: &'static str = "mesh.media_progress";
}

impl Topic for ThereState {
    const TOPIC: &'static str = "media.there";
    const RETAINED: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn media_messages_round_trip() {
        let open = MediaOpen {
            device: "d".into(),
            title: "فيروز".into(),
            artist: "x".into(),
        };
        assert_eq!(decode::<MediaOpen>(&encode(&open).unwrap()).unwrap(), open);
        let ctl = MediaCtlOut {
            device: "d".into(),
            id: 4,
            ctl: MediaCtl::Volume(35),
        };
        assert_eq!(decode::<MediaCtlOut>(&encode(&ctl).unwrap()).unwrap(), ctl);
        let state = ThereState {
            outputs: vec![ThereOutput {
                device: "d".into(),
                name: "Desk".into(),
            }],
            active: Some("d".into()),
            paused: true,
            volume: 50,
            playing_for: None,
        };
        assert_eq!(
            decode::<ThereState>(&encode(&state).unwrap()).unwrap(),
            state
        );
    }
}
