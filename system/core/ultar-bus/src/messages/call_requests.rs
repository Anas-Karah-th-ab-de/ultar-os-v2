//! Requests of the call service and their answers (part of [`super`], the `call` contract).

use super::{CallState, Direction, Output, Way, SERVICE};
use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// What happens to the other call when a waiting call is answered.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Others {
    /// Hold the other call.
    Hold,
    /// End the other call.
    End,
}

/// One entry of the call log.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct LogEntry {
    /// Identifier (stable while the entry exists).
    pub id: u32,
    /// The way.
    pub way: Way,
    /// Who called.
    pub direction: Direction,
    /// Not answered (incoming).
    pub missed: bool,
    /// The other number.
    pub number: String,
    /// Name, if known.
    pub name: Option<String>,
    /// Start time, seconds since the Unix epoch (wall clock of the service).
    pub when_s: i64,
}

macro_rules! calls {
    ($($call:ident => $method:literal, $reply:ident;)*) => {$(
        impl Message for $call {
            const VERSION: u16 = 1;
        }
        impl Call for $call {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $method;
            type Reply = $reply;
        }
    )*};
}

/// Plain answer of a call that only says "done".
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Done;

/// `call.dial` – start a call.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Dial {
    /// The way.
    pub way: Way,
    /// The number (with `UOS-` for the Ultar way).
    pub number: String,
    /// Video call (Ultar way only).
    pub video: bool,
}

/// Answer to [`Dial`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct DialReply {
    /// Identifier of the new call.
    pub id: u32,
}

/// `call.answer` – accept a ringing or waiting call.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Answer {
    /// The call.
    pub id: u32,
    /// What happens to another live call.
    pub others: Others,
}

/// `call.decline` – decline a ringing or waiting call.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Decline {
    /// The call.
    pub id: u32,
}

/// `call.hangup` – end a call (any live phase).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hangup {
    /// The call.
    pub id: u32,
}

/// `call.hold` – hold (`on`) or resume an active call; resuming holds the other one.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hold {
    /// The call.
    pub id: u32,
    /// Hold or resume.
    pub on: bool,
}

/// `call.mute` – mute or unmute the microphone.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mute {
    /// Muted.
    pub on: bool,
}

/// `call.output` – choose the audio output.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetOutput {
    /// The output (must be in [`Audio::outputs`]).
    pub output: Output,
}

/// `call.dtmf` – send one tone (`0-9`, `*`, `#`) in an active call.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dtmf {
    /// The call.
    pub id: u32,
    /// The key as ASCII.
    pub key: u8,
}

/// `call.video` – own camera on or off (Ultar way).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetVideo {
    /// The call.
    pub id: u32,
    /// Camera on.
    pub on: bool,
}

/// `call.camera` – switch to the next camera.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct NextCamera {
    /// The call.
    pub id: u32,
}

/// `call.share` – start or stop sharing the screen (after the system permission prompt).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Share {
    /// The call.
    pub id: u32,
    /// Share.
    pub on: bool,
}

/// `call.text` – send a short message to a number (decline with message).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SendText {
    /// The way.
    pub way: Way,
    /// Receiver.
    pub number: String,
    /// The message (written by the user or chosen from the ready messages).
    pub text: String,
}

/// `call.log` – the call log, newest first.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Log {
    /// Only missed calls.
    pub missed_only: bool,
}

/// Answer to [`Log`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct LogReply {
    /// Entries, newest first.
    pub entries: Vec<LogEntry>,
}

/// `call.forget` – delete one log entry.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Forget {
    /// Entry identifier.
    pub id: u32,
}

/// `call.clear` – delete the whole call history (blocked numbers stay).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClearLog;

/// `call.block` – block (`on`) or unblock a number; calls from it are declined unheard.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Block {
    /// The number.
    pub number: String,
    /// Block or unblock.
    pub on: bool,
}

/// `call.snapshot` – the current [`CallState`] (same as the retained topic).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Snapshot;

impl Message for Done {
    const VERSION: u16 = 1;
}
impl Message for DialReply {
    const VERSION: u16 = 1;
}
impl Message for LogReply {
    const VERSION: u16 = 1;
}
impl Message for CallState {
    const VERSION: u16 = 1;
}
impl Topic for CallState {
    const TOPIC: &'static str = "call.state";
    const RETAINED: bool = true;
}

calls! {
    Dial => "dial", DialReply;
    Answer => "answer", Done;
    Decline => "decline", Done;
    Hangup => "hangup", Done;
    Hold => "hold", Done;
    Mute => "mute", Done;
    SetOutput => "output", Done;
    Dtmf => "dtmf", Done;
    SetVideo => "video", Done;
    NextCamera => "camera", Done;
    Share => "share", Done;
    SendText => "text", Done;
    Log => "log", LogReply;
    Forget => "forget", Done;
    ClearLog => "clear", Done;
    Block => "block", Done;
    Snapshot => "snapshot", CallState;
}
