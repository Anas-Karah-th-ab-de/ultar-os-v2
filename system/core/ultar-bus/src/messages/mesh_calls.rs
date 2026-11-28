//! Contract of the device mesh for calls, sound and notifications (`mesh`-Punkt C2/C3:
//! "ein Anruf klingelt auf dem Schreibtisch mit und kann dort geführt werden", "Ton und
//! Hinweise wandern mit").
//!
//! A call that rings on the phone can be offered to a paired device, answered and talked
//! there and hung up from either side; the voice travels as 20 ms frames of PCM 16-bit,
//! 48 kHz mono over the encrypted mesh channel, the phone keeps the SIM/Id call leg.
//! Notifications of the phone can be mirrored to the paired devices. Every part is gated
//! by [`crate::messages::mesh::Right::Call`], which is off after pairing.

use crate::message::{Call, Message, Topic};
use crate::messages::mesh::{Done, SERVICE};
use crate::messages::notify::Urgency;
use serde::{Deserialize, Serialize};

/// `mesh.call_offer` – offer a call that rings here to one paired device (the phone's
/// call service names the device).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct CallOffer {
    /// The device to offer the call to.
    pub device: String,
    /// Who calls – only what the phone already shows (name or number).
    pub caller: String,
    /// It was placed as a video call.
    pub video: bool,
}

/// Answer to [`CallOffer`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct CallOffered {
    /// Id of the offer on this device (`0`: the device could not take the call).
    pub id: u64,
    /// The device it went to (empty when nothing was sent).
    pub device: String,
}

/// `mesh.call_incoming` – a paired device offers a call here (topic, not retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct CallIncoming {
    /// Id of the offer on this device.
    pub id: u64,
    /// The device that offers.
    pub from: String,
    /// Its name (data).
    pub from_name: String,
    /// Who calls.
    pub caller: String,
    /// Video call.
    pub video: bool,
}

/// `mesh.call_answer` – answer or decline the offered call (the shell here).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallAnswer {
    /// The offer.
    pub id: u64,
    /// Accepted.
    pub accept: bool,
}

/// `mesh.call_end` – hang up on this device (either side).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallEnd {
    /// The call.
    pub id: u64,
}

/// Where an offered call stands on a device.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallPhase {
    /// Waiting for the person there to answer.
    Offered,
    /// Answered; the voice flows.
    Accepted,
    /// The person there declined.
    Declined,
    /// Hung up on either side.
    Ended,
    /// The device is gone or could not be reached.
    Failed,
}

/// `mesh.call_progress` – a call changed on this device (topic, not retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct CallProgress {
    /// The offer.
    pub id: u64,
    /// The other device.
    pub device: String,
    /// Where it stands.
    pub phase: CallPhase,
}

/// `mesh.call_audio` – one 20 ms of the call's voice arrived from a paired device
/// (topic, not retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct CallAudio {
    /// The offer.
    pub id: u64,
    /// Frame counter (for the jitter buffer).
    pub seq: u64,
    /// PCM 16-bit, 48 kHz mono, base64 (see `ultar_mesh::audio_link`).
    pub pcm: String,
    /// The device the frame came from.
    pub device: String,
}

/// `mesh.call_audio` (method) – send one voice frame to a paired device (the local call
/// service).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct CallAudioIn {
    /// The device the frame goes to.
    pub device: String,
    /// The offer.
    pub id: u64,
    /// Frame counter.
    pub seq: u64,
    /// PCM 16-bit, 48 kHz mono, base64.
    pub pcm: String,
}

/// `mesh.note_mirror` – mirror a notification of this device to the paired devices
/// (the notifyd reports it).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NoteMirror {
    /// Title as shown here (data).
    pub title: String,
    /// Body (data).
    pub text: String,
    /// Application id of the sender.
    pub app: String,
    /// Attention level.
    pub urgency: Urgency,
    /// Never leaves this device (a password, a one-time code).
    pub confidential: bool,
    /// Only show on a target that is unlocked.
    pub hide_on_locked: bool,
}

/// `mesh.note` – a notification mirrored from another device (topic, not retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NoteRemote {
    /// The device it came from.
    pub from: String,
    /// Its name (data).
    pub from_name: String,
    /// Title (data).
    pub title: String,
    /// Body (data).
    pub text: String,
    /// Application id of the sender.
    pub app: String,
    /// Attention level.
    pub urgency: Urgency,
    /// The source asks to show it only on an unlocked target.
    pub hide_on_locked: bool,
}

/// `call.mesh_mute` - mute or unmute the microphone of a call that is led here for another
/// device (the shell on the device that answered; the phone's own `call.mute` is separate).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallMeshMute {
    /// Muted.
    pub on: bool,
}

/// `call.mesh` - the call this device leads for another device (retained). `id` is `None`
/// while there is none.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct MeshCallState {
    /// The offer that is answered here.
    pub id: Option<u64>,
    /// The device that has the call's line.
    pub device: String,
    /// The microphone is muted.
    pub muted: bool,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    CallMeshMute,
    MeshCallState,
    CallOffer,
    CallOffered,
    CallIncoming,
    CallAnswer,
    CallEnd,
    CallPhase,
    CallProgress,
    CallAudio,
    CallAudioIn,
    NoteMirror,
    NoteRemote
);

macro_rules! call {
    ($t:ty, $m:literal, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
call!(CallOffer, "call_offer", CallOffered);
call!(CallAnswer, "call_answer", Done);
call!(CallEnd, "call_end", Done);
call!(CallAudioIn, "call_audio", Done);
call!(NoteMirror, "note_mirror", Done);

impl Call for CallMeshMute {
    const SERVICE: &'static str = crate::messages::call::SERVICE;
    const METHOD: &'static str = "mesh_mute";
    type Reply = Done;
}

impl Topic for MeshCallState {
    const TOPIC: &'static str = "call.mesh";
    const RETAINED: bool = true;
}

impl Topic for CallIncoming {
    const TOPIC: &'static str = "mesh.call_incoming";
}

impl Topic for CallProgress {
    const TOPIC: &'static str = "mesh.call_progress";
}

impl Topic for CallAudio {
    const TOPIC: &'static str = "mesh.call_audio";
}

impl Topic for NoteRemote {
    const TOPIC: &'static str = "mesh.note";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn call_messages_round_trip() {
        let offer = CallOffer {
            device: "device".into(),
            caller: "+963 000".into(),
            video: false,
        };
        assert_eq!(
            decode::<CallOffer>(&encode(&offer).unwrap()).unwrap(),
            offer
        );
        let incoming = CallIncoming {
            id: 3,
            from: "abc".into(),
            from_name: "Phone".into(),
            caller: "Anas".into(),
            video: true,
        };
        assert_eq!(
            decode::<CallIncoming>(&encode(&incoming).unwrap()).unwrap(),
            incoming
        );
        let audio = CallAudio {
            id: 3,
            seq: 12,
            pcm: "aGVsbG8".into(),
            device: "abc".into(),
        };
        assert_eq!(
            decode::<CallAudio>(&encode(&audio).unwrap()).unwrap(),
            audio
        );
    }

    #[test]
    fn note_messages_round_trip() {
        let mirror = NoteMirror {
            title: "SMS".into(),
            text: "مرحبا".into(),
            app: "tech.code-craft.messages".into(),
            urgency: Urgency::High,
            confidential: true,
            hide_on_locked: true,
        };
        assert_eq!(
            decode::<NoteMirror>(&encode(&mirror).unwrap()).unwrap(),
            mirror
        );
        let remote = NoteRemote {
            from: "abc".into(),
            from_name: "Phone".into(),
            title: "SMS".into(),
            text: "مرحبا".into(),
            app: "tech.code-craft.messages".into(),
            urgency: Urgency::Normal,
            hide_on_locked: false,
        };
        assert_eq!(
            decode::<NoteRemote>(&encode(&remote).unwrap()).unwrap(),
            remote
        );
    }

    #[test]
    fn call_phases_cover_every_step() {
        assert_eq!(
            decode::<CallPhase>(&encode(&CallPhase::Offered).unwrap()).unwrap(),
            CallPhase::Offered
        );
        assert_eq!(
            decode::<CallPhase>(&encode(&CallPhase::Ended).unwrap()).unwrap(),
            CallPhase::Ended
        );
    }
}
