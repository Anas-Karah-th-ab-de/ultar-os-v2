//! What two devices say to each other inside the encrypted channel.
//!
//! One frame is one JSON object with a `t` tag. A frame with a tag this version does not
//! know decodes as [`Frame::Unknown`] and is ignored, so newer devices can say more.

use serde::{Deserialize, Serialize};
use ultar_bus::messages::agent::Step;

/// Version of this protocol (sent in [`Frame::Hello`]).
pub const VERSION: u16 = 1;

/// Largest frame in bytes (one Noise message carries one frame).
pub const MAX_FRAME: usize = 60_000;

/// Bytes of a file in one [`Frame::Chunk`].
pub const CHUNK: usize = 32 * 1024;

/// A file that travels with an offer.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct FileMeta {
    /// Its name without any folder (data).
    pub name: String,
    /// Its size in bytes.
    pub size: u64,
    /// SHA-256 of its content, hexadecimal.
    pub sha256: String,
}

/// One message between two devices.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Frame {
    /// First frame of both sides: who I am and what I can do.
    Hello {
        /// Protocol version.
        version: u16,
        /// Name (data).
        name: String,
        /// Kind as a word.
        kind: String,
        /// Capabilities as words.
        caps: Vec<String>,
        /// Ids of installed apps.
        apps: Vec<String>,
    },
    /// Pairing, step 1 (the device that typed the code).
    PairStart {
        /// Its key-exchange message.
        msg: String,
    },
    /// Pairing, step 2 (the device that shows the code).
    PairReply {
        /// Its key-exchange message.
        msg: String,
        /// Proof that it derived the same key.
        proof: String,
    },
    /// Pairing, step 3.
    PairConfirm {
        /// Proof of the device that typed the code.
        proof: String,
    },
    /// Pairing was refused (wrong code, no code shown, too many attempts).
    PairRefused,
    /// "Continue this here?" – or a plain file.
    Offer {
        /// Id chosen by the sender.
        id: u64,
        /// Title (data).
        title: String,
        /// App id (empty for a plain file).
        app: String,
        /// How it arrives: `state`, `file` or `remote_view`.
        stage: String,
        /// What to open.
        uri: Option<String>,
        /// The file that follows once accepted.
        file: Option<FileMeta>,
    },
    /// The person's answer to an offer.
    Answer {
        /// The offer.
        id: u64,
        /// Accepted.
        accept: bool,
    },
    /// A piece of the file of an accepted offer.
    Chunk {
        /// The offer.
        id: u64,
        /// Number of the piece, from 0.
        index: u32,
        /// The bytes.
        data: String,
    },
    /// The receiver has piece `index` and wants the next.
    ChunkAck {
        /// The offer.
        id: u64,
        /// The piece it has.
        index: u32,
    },
    /// How an accepted offer ended on the receiver: `opened` or `failed`.
    Outcome {
        /// The offer.
        id: u64,
        /// The result.
        state: String,
    },
    /// The sender takes an offer back.
    Cancel {
        /// The offer.
        id: u64,
    },
    /// Text copied on the sender.
    Clip {
        /// Counter of the sender's clipboard.
        seq: u64,
        /// The text.
        text: String,
    },
    /// One piece of a picture copied on the sender (`crate::clip_image`).
    ClipImage(crate::clip_image::Piece),
    /// Run a program (computing jobs).
    Job {
        /// Id chosen by the sender; a repeated id is answered from memory.
        id: String,
        /// The program.
        program: String,
        /// Its arguments.
        args: Vec<String>,
    },
    /// A job is still running.
    JobAlive {
        /// The job.
        id: String,
    },
    /// A job ended.
    JobDone {
        /// The job.
        id: String,
        /// Its exit code (`None`: it could not run).
        code: Option<i32>,
        /// What it printed (cut to fit one frame).
        output: String,
    },
    /// The device takes no job right now, or may not.
    JobRefused {
        /// The job.
        id: String,
    },
    /// Let your language model answer this (lending the model).
    Think {
        /// Id chosen by the asking device.
        id: u64,
        /// What the model is and may do.
        system: String,
        /// What it is asked.
        prompt: String,
        /// Longest answer in tokens.
        max_tokens: u32,
    },
    /// The answer to a [`Frame::Think`].
    Thought {
        /// The request.
        id: u64,
        /// The model answered.
        ok: bool,
        /// Its answer.
        text: String,
    },
    /// Let your helper do this (acting on another device).
    Act {
        /// Id chosen by the asking device.
        id: u64,
        /// The request, in the person's words.
        text: String,
    },
    /// The answer to a [`Frame::Act`].
    Acted {
        /// The request.
        id: u64,
        /// The helper took it.
        ok: bool,
        /// What it did.
        steps: Vec<Step>,
    },
    /// See this device's screen (remote view): a viewer asks a source to stream its
    /// picture and take its pointer and keys.
    ViewStart {
        /// Session id chosen by the viewer.
        id: u64,
    },
    /// The source refuses (no right, busy, or its person said no).
    ViewRefuse {
        /// The session.
        id: u64,
    },
    /// The source accepts and will stream `width` × `height` pixels.
    ViewAccept {
        /// The session.
        id: u64,
        /// Picture width in pixels.
        width: u32,
        /// Picture height in pixels.
        height: u32,
    },
    /// One damaged rectangle of the picture, QOI-compressed (base64).
    ViewFrame {
        /// The session.
        id: u64,
        /// Rising counter; the viewer acknowledges the frames it has rendered.
        seq: u32,
        /// `x, y, width, height` of the rectangle in pixels.
        region: [u32; 4],
        /// The pixels of that rectangle, QOI-compressed, base64.
        data: String,
    },
    /// The viewer has rendered everything up to (and including) `seq` (flow control).
    ViewAck {
        /// The session.
        id: u64,
        /// Sequence rendered.
        seq: u32,
    },
    /// The viewer's pointer and keys on the source (a short word code, see
    /// `ultar-screend`).
    ViewInput {
        /// The session.
        id: u64,
        /// The event.
        input: String,
    },
    /// Either side ends the session.
    ViewStop {
        /// The session.
        id: u64,
        /// The source ended it because its screen locked.
        #[serde(default)]
        locked: bool,
    },
    /// Offer a call that rings here (who calls is only what the phone shows).
    Call {
        /// Id chosen by the offering device.
        id: u64,
        /// Name or number of the caller.
        caller: String,
        /// Video call.
        video: bool,
    },
    /// The person's answer to a [`Frame::Call`].
    CallAnswer {
        /// The call.
        id: u64,
        /// Accepted.
        accept: bool,
    },
    /// Hang up (either side).
    CallEnd {
        /// The call.
        id: u64,
    },
    /// One 20 ms of the call's voice: PCM 16-bit, 48 kHz mono, base64.
    CallAudio {
        /// The call.
        id: u64,
        /// Frame counter.
        seq: u64,
        /// The PCM bytes.
        pcm: String,
    },
    /// A notification of this device to mirror on the other.
    Note {
        /// Counter of the sender's notifications.
        seq: u64,
        /// Title (data).
        title: String,
        /// Body (data).
        text: String,
        /// Application id of the sender.
        app: String,
        /// Attention level: 0 low, 1 normal, 2 high.
        urgency: u8,
        /// The source asks to show it only on an unlocked target.
        hide_on_locked: bool,
    },
    /// Start playing the sound of a media session here ("play there").
    MediaOpen {
        /// Id chosen by the source.
        id: u64,
        /// What plays (data).
        title: String,
        /// By whom (data).
        artist: String,
    },
    /// One 20 ms of the media sound: PCM 16-bit, 48 kHz mono, base64.
    MediaPcm {
        /// The session.
        id: u64,
        /// Frame counter.
        seq: u64,
        /// The PCM bytes.
        pcm: String,
    },
    /// Pause, resume, volume or stop (either end).
    MediaCtl {
        /// The session.
        id: u64,
        /// What to do.
        ctl: ultar_bus::messages::mesh_media::MediaCtl,
    },
    /// Anything this version does not know.
    #[serde(other)]
    Unknown,
}

/// A frame as bytes; `None` when it does not fit [`MAX_FRAME`].
pub fn pack(frame: &Frame) -> Option<Vec<u8>> {
    let bytes = serde_json::to_vec(frame).ok()?;
    (bytes.len() <= MAX_FRAME).then_some(bytes)
}

/// Bytes as a frame; `None` when they are no frame at all.
pub fn unpack(bytes: &[u8]) -> Option<Frame> {
    if bytes.len() > MAX_FRAME {
        return None;
    }
    serde_json::from_slice(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_survive_the_wire() {
        let frame = Frame::Offer {
            id: 7,
            title: "notes.txt".into(),
            app: "editor".into(),
            stage: "state".into(),
            uri: Some("file:///home/a/notes.txt#line=12".into()),
            file: None,
        };
        let bytes = pack(&frame).unwrap();
        assert_eq!(unpack(&bytes), Some(frame));
    }

    #[test]
    fn a_frame_of_a_newer_version_is_ignored_not_refused() {
        assert_eq!(
            unpack(br#"{"t":"cast_video","url":"x"}"#),
            Some(Frame::Unknown)
        );
        assert_eq!(unpack(b"not json"), None);
    }

    #[test]
    fn a_full_chunk_fits_one_frame() {
        let data = crate::device::encode(&vec![0xAB; CHUNK]);
        let frame = Frame::Chunk {
            id: u64::MAX,
            index: u32::MAX,
            data,
        };
        assert!(pack(&frame).is_some());
        let huge = Frame::Clip {
            seq: 1,
            text: "x".repeat(MAX_FRAME),
        };
        assert!(pack(&huge).is_none());
    }

    #[test]
    fn a_view_frame_survives_the_wire() {
        let frame = Frame::ViewFrame {
            id: 3,
            seq: 9,
            region: [2, 3, 8, 6],
            data: "AAAA".into(),
        };
        let bytes = pack(&frame).unwrap();
        assert_eq!(unpack(&bytes), Some(frame));
    }

    #[test]
    fn call_and_note_frames_survive_the_wire() {
        let call = Frame::Call {
            id: 9,
            caller: "+963 000".into(),
            video: false,
        };
        assert_eq!(unpack(&pack(&call).unwrap()), Some(call));
        let audio = Frame::CallAudio {
            id: 9,
            seq: 0xCAFE,
            pcm: "aGVsbG8=".into(),
        };
        assert_eq!(unpack(&pack(&audio).unwrap()), Some(audio));
        let note = Frame::Note {
            seq: 4,
            title: "SMS".into(),
            text: "مرحبا".into(),
            app: "messages".into(),
            urgency: 2,
            hide_on_locked: true,
        };
        assert_eq!(unpack(&pack(&note).unwrap()), Some(note));
    }
}
