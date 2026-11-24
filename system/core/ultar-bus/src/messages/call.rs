//! Contracts of the call service (`ultar-call`): voice and video calls over the two ways of
//! DESKTOP-SPEC §12.2 – the Ultar number (end-to-end encrypted, over the cloud signalling)
//! and the mobile network (SIM 1 / SIM 2, only with a modem).
//!
//! | Name | Kind | Retained | Owner |
//! |---|---|---|---|
//! | `call.state` | topic | yes | call service |
//! | `call.dial` / `answer` / `decline` / `hangup` / `hold` / `mute` / `output` / `dtmf` | calls | – | call service |
//! | `call.video` / `camera` / `share` / `text` / `log` / `forget` / `block` / `snapshot` | calls | – | call service |
//!
//! Failures travel as [`crate::ErrorCode::Service`] with the codes of [`CallError`].
//! Numbers and names are data typed by people (never interface text); every limit below is
//! enforced by the service and checked by [`check_number`] / [`check_text`].

use serde::{Deserialize, Serialize};

/// Service name of the call service.
pub const SERVICE: &str = "call";
/// At most this many calls at once (one active or held plus one waiting / held).
pub const MAX_CALLS: usize = 2;
/// Longest number in bytes (`UOS-XXXX-XXXX`, `+963 11 555 0142`, USSD codes).
pub const MAX_NUMBER_LEN: usize = 32;
/// Longest display name in bytes.
pub const MAX_NAME_LEN: usize = 64;
/// Longest ready message ("decline with message") in bytes.
pub const MAX_TEXT_LEN: usize = 160;
/// Most entries the call log keeps (oldest dropped first).
pub const MAX_LOG: usize = 200;
/// Most blocked numbers.
pub const MAX_BLOCKED: usize = 256;
/// Prefix of every Ultar number.
pub const ULTAR_PREFIX: &str = "UOS-";

/// Emergency numbers that are dialled on the mobile network even without SIM or service.
pub const EMERGENCY_NUMBERS: [&str; 8] = ["112", "911", "999", "110", "113", "000", "118", "119"];

/// The way a call travels.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Way {
    /// Ultar number: end-to-end encrypted over the cloud signalling.
    Ultar,
    /// Mobile network through the SIM in this slot (1 or 2).
    Cellular(u8),
}

/// Availability of the mobile network way.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Cellular {
    /// The device has no modem.
    #[default]
    NoModem,
    /// No SIM card is inserted.
    NoSim,
    /// Airplane mode: the radio is off.
    Airplane,
    /// SIM present, but no network found.
    NoNetwork,
    /// Only emergency calls are possible.
    EmergencyOnly,
    /// Registered: calls possible.
    Ready,
}

/// Availability of the Ultar-number way.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Ultar {
    /// No Ultar account on this device.
    #[default]
    SignedOut,
    /// Signed in, but the signalling server cannot be reached.
    Offline,
    /// Registered at the signalling server.
    Ready,
}

/// Both ways and the own number.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Lines {
    /// Mobile network.
    pub cellular: Cellular,
    /// SIM slots with a card (subset of `[1, 2]`).
    pub sims: Vec<u8>,
    /// Ultar number way.
    pub ultar: Ultar,
    /// Own Ultar number (`UOS-XXXX-XXXX`) when signed in.
    pub own_number: Option<String>,
}

/// Who started the call.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// The other side called.
    Incoming,
    /// This device called.
    Outgoing,
}

/// Why a call could not be made or broke off.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failure {
    /// No mobile network.
    NoNetwork,
    /// No SIM card.
    NoSim,
    /// Airplane mode is on.
    Airplane,
    /// Only emergency calls are possible.
    EmergencyOnly,
    /// No Ultar account.
    SignedOut,
    /// Signalling server not reachable.
    Offline,
    /// The number does not exist or cannot be reached.
    Unreachable,
    /// The other Ultar device is offline.
    PeerOffline,
    /// Microphone or camera could not be opened (right missing or device busy).
    Media,
    /// The network refused the call.
    Rejected,
}

/// How a call ended.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum End {
    /// Hung up on this device.
    Local,
    /// The other side hung up.
    Remote,
    /// Declined on this device.
    Declined,
    /// Incoming call that nobody answered.
    Missed,
    /// The other side is busy.
    Busy,
    /// From a blocked number: declined without ringing.
    Blocked,
    /// It failed.
    Failed(Failure),
}

/// Phase of one call.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Outgoing, the network sets it up.
    Dialing,
    /// Outgoing, the other side rings.
    Alerting,
    /// Incoming, rings here (no other call).
    Ringing,
    /// Incoming while another call is going on (second incoming call).
    Waiting,
    /// Talking.
    Active,
    /// On hold.
    Held,
    /// Over (only in [`CallState::ended`]).
    Ended(End),
}

impl Phase {
    /// True for a call that still rings here (ringing or waiting).
    pub fn is_incoming(self) -> bool {
        matches!(self, Phase::Ringing | Phase::Waiting)
    }

    /// True while the call exists and is not over.
    pub fn is_live(self) -> bool {
        !matches!(self, Phase::Ended(_))
    }
}

/// Where the call is heard.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Output {
    /// Earpiece (phone at the ear) or the default output on a PC.
    #[default]
    Earpiece,
    /// Loudspeaker.
    Speaker,
    /// Wired headset.
    Headset,
    /// Bluetooth headset.
    Bluetooth,
}

/// One call.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct CallInfo {
    /// Identifier, unique for the life of the service.
    pub id: u32,
    /// The way.
    pub way: Way,
    /// Who called.
    pub direction: Direction,
    /// The other number.
    pub number: String,
    /// Name of the other side from the contacts, if known.
    pub name: Option<String>,
    /// Phase.
    pub phase: Phase,
    /// Media of the call: video, camera and screen sharing.
    pub media: Media,
    /// End-to-end encrypted (always for the Ultar way).
    pub encrypted: bool,
    /// Service clock (ms) when the call became active; the duration is `clock_ms - this`.
    pub active_since_ms: Option<u64>,
}

/// Video state of a call.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Media {
    /// The call was placed or answered as a video call.
    pub video_call: bool,
    /// Own camera sends.
    pub camera_on: bool,
    /// Index of the camera in use.
    pub camera: u8,
    /// Number of cameras.
    pub cameras: u8,
    /// The screen is shared.
    pub sharing: bool,
}

/// Audio of the calls (one microphone, one output for all calls).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Audio {
    /// Microphone muted.
    pub muted: bool,
    /// Current output.
    pub output: Output,
    /// Outputs that exist now.
    pub outputs: Vec<Output>,
}

/// `call.state` – everything a phone surface shows. Retained.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct CallState {
    /// Availability of both ways.
    pub lines: Lines,
    /// Live calls (at most [`MAX_CALLS`]), the oldest first.
    pub calls: Vec<CallInfo>,
    /// The call that ended last (phase `Ended`), until the next call starts.
    pub ended: Option<CallInfo>,
    /// Audio.
    pub audio: Audio,
    /// Service clock in milliseconds when this state was made.
    pub clock_ms: u64,
}

/// Error codes of the call service (sent as `ErrorCode::Service(code)`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallError {
    /// No live call has this identifier.
    UnknownCall,
    /// The number is empty, too long or has characters a number cannot have.
    BadNumber,
    /// A text is too long or empty.
    BadText,
    /// Already [`MAX_CALLS`] calls.
    TooManyCalls,
    /// Not possible in the call's phase (e.g. hold while ringing).
    WrongPhase,
    /// The way cannot do this (video on the mobile network, tones on Ultar).
    Unsupported,
    /// The way is not available now.
    Unavailable(Failure),
    /// The backend refused or broke.
    Backend,
    /// The list of blocked numbers is full.
    Full,
}

impl CallError {
    /// Wire code of the error.
    pub fn code(self) -> u32 {
        match self {
            CallError::UnknownCall => 1,
            CallError::BadNumber => 2,
            CallError::BadText => 3,
            CallError::TooManyCalls => 4,
            CallError::WrongPhase => 5,
            CallError::Unsupported => 6,
            CallError::Backend => 7,
            CallError::Full => 8,
            CallError::Unavailable(f) => 100 + failure_index(f),
        }
    }

    /// Error of a wire code (`None` for a code this contract does not define).
    pub fn from_code(code: u32) -> Option<CallError> {
        Some(match code {
            1 => CallError::UnknownCall,
            2 => CallError::BadNumber,
            3 => CallError::BadText,
            4 => CallError::TooManyCalls,
            5 => CallError::WrongPhase,
            6 => CallError::Unsupported,
            7 => CallError::Backend,
            8 => CallError::Full,
            100..=109 => CallError::Unavailable(FAILURES[(code - 100) as usize]),
            _ => return None,
        })
    }
}

const FAILURES: [Failure; 10] = [
    Failure::NoNetwork,
    Failure::NoSim,
    Failure::Airplane,
    Failure::EmergencyOnly,
    Failure::SignedOut,
    Failure::Offline,
    Failure::Unreachable,
    Failure::PeerOffline,
    Failure::Media,
    Failure::Rejected,
];

fn failure_index(f: Failure) -> u32 {
    FAILURES.iter().position(|x| *x == f).unwrap_or(0) as u32
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "call service error {}", self.code())
    }
}

impl std::error::Error for CallError {}

/// True for a number that is dialled as an emergency call.
pub fn is_emergency(number: &str) -> bool {
    EMERGENCY_NUMBERS.contains(&number.trim())
}

/// Checks a number: 1..=[`MAX_NUMBER_LEN`] bytes of digits, `+`, `*`, `#`, `-`, spaces and
/// (for Ultar numbers) the letters of the prefix.
pub fn check_number(number: &str) -> Result<(), CallError> {
    let ok_char = |c: char| c.is_ascii_digit() || "+*#- ".contains(c) || "UOS".contains(c);
    let digits = number.chars().filter(char::is_ascii_digit).count();
    if number.is_empty() || number.len() > MAX_NUMBER_LEN || !number.chars().all(ok_char) {
        return Err(CallError::BadNumber);
    }
    if digits == 0 && !number.contains(['*', '#']) {
        return Err(CallError::BadNumber);
    }
    Ok(())
}

/// Checks a ready message: 1..=[`MAX_TEXT_LEN`] bytes, no control characters.
pub fn check_text(text: &str) -> Result<(), CallError> {
    if text.trim().is_empty() || text.len() > MAX_TEXT_LEN || text.chars().any(char::is_control) {
        return Err(CallError::BadText);
    }
    Ok(())
}

#[path = "call_requests.rs"]
mod requests;
pub use requests::*;

#[cfg(test)]
#[path = "call_tests.rs"]
mod tests;
