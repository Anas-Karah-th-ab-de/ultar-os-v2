//! Contracts of the modem service (`ultar-ril --daemon`): the calls of the mobile network as
//! the modem lists them, and the commands the call service sends (dial, answer, release, hold,
//! tones, microphone). Only the call service may use them (policy).
//!
//! | Name | Kind | Retained | Owner |
//! |---|---|---|---|
//! | `modem.state` | topic | yes | modem service |
//! | `modem.dial` / `answer` / `release` / `hold` / `dtmf` / `mute` | calls | – | modem service |
//!
//! Failures: `ErrorCode::Service(1)` no modem / radio off, `2` the modem refused the command.

use super::call::Cellular;
use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Service name of the modem service.
pub const SERVICE: &str = "modem";
/// Error code: no modem, or the radio is off.
pub const ERR_OFFLINE: u32 = 1;
/// Error code: the modem refused or failed the command.
pub const ERR_REFUSED: u32 = 2;
/// Most calls the modem lists.
pub const MAX_MODEM_CALLS: usize = 7;

/// Status of one call as the modem lists it.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModemCallStatus {
    /// Outgoing, being set up.
    Dialing,
    /// Outgoing, rings at the other side.
    Alerting,
    /// Incoming, rings here.
    Incoming,
    /// Incoming while another call is going on.
    Waiting,
    /// Talking.
    Active,
    /// On hold.
    Held,
}

/// One call of the modem.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ModemCall {
    /// Call index of the modem (1–7).
    pub index: u8,
    /// Incoming (`true`) or outgoing.
    pub incoming: bool,
    /// The other number.
    pub number: String,
    /// Status.
    pub status: ModemCallStatus,
}

/// `modem.state` – availability and the calls of the modem. Retained.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ModemState {
    /// Availability of the mobile network.
    pub cellular: Cellular,
    /// SIM slots with a card.
    pub sims: Vec<u8>,
    /// Calls, at most [`MAX_MODEM_CALLS`].
    pub calls: Vec<ModemCall>,
}

/// Answer of every modem command.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ModemDone;

/// `modem.dial` – call a number (the call appears in `modem.state`).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ModemDial {
    /// The number.
    pub number: String,
}

/// `modem.answer` – accept the ringing call.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ModemAnswer;

/// `modem.release` – end or decline a call.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModemRelease {
    /// Call index.
    pub index: u8,
}

/// `modem.hold` – hold (`on`) or resume a call (the modem swaps active and held calls).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModemHold {
    /// Call index.
    pub index: u8,
    /// Hold or resume.
    pub on: bool,
}

/// `modem.dtmf` – one tone (`0-9`, `*`, `#`) in the active call.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModemDtmf {
    /// The key as ASCII.
    pub key: u8,
}

/// `modem.mute` – microphone of the modem's voice path.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModemMute {
    /// Muted.
    pub on: bool,
}

/// `modem.set_data` – mobile data wanted (`Some`) or only asked (`None`). The radio service
/// keeps the wish and builds the data connection as soon as SIM and network allow it.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModemSetData {
    /// On, off, or `None` to ask.
    pub on: Option<bool>,
}

/// Answer to [`ModemSetData`]: whether mobile data is wanted now.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ModemData {
    /// Mobile data wanted.
    pub on: bool,
}

impl Message for ModemSetData {
    const VERSION: u16 = 1;
}
impl Message for ModemData {
    const VERSION: u16 = 1;
}
impl Call for ModemSetData {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "set_data";
    type Reply = ModemData;
}

macro_rules! modem_calls {
    ($($call:ident => $method:literal;)*) => {$(
        impl Message for $call {
            const VERSION: u16 = 1;
        }
        impl Call for $call {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $method;
            type Reply = ModemDone;
        }
    )*};
}

modem_calls! {
    ModemDial => "dial";
    ModemAnswer => "answer";
    ModemRelease => "release";
    ModemHold => "hold";
    ModemDtmf => "dtmf";
    ModemMute => "mute";
}

impl Message for ModemDone {
    const VERSION: u16 = 1;
}
impl Message for ModemState {
    const VERSION: u16 = 1;
}
impl Topic for ModemState {
    const TOPIC: &'static str = "modem.state";
    const RETAINED: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};
    use crate::topic::{is_valid_name, is_valid_segment_name};

    #[test]
    fn names_are_valid_and_state_round_trips() {
        for m in [
            ModemDial::METHOD,
            ModemAnswer::METHOD,
            ModemRelease::METHOD,
            ModemHold::METHOD,
            ModemDtmf::METHOD,
            ModemMute::METHOD,
        ] {
            assert!(is_valid_segment_name(m));
        }
        assert!(is_valid_name(ModemState::TOPIC) && is_valid_name(SERVICE));
        let st = ModemState {
            cellular: Cellular::Ready,
            sims: vec![1],
            calls: vec![ModemCall {
                index: 1,
                incoming: true,
                number: "0933".into(),
                status: ModemCallStatus::Waiting,
            }],
        };
        assert_eq!(decode::<ModemState>(&encode(&st).unwrap()).unwrap(), st);
    }
}
