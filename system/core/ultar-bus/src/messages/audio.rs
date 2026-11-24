//! Contract of the audio service: volume, mute and the output of the device.
//!
//! On a device without sound card the state says `present: false` and every change
//! answers [`AudioOutcome::NoDevice`] – the shell shows "no sound device", never a
//! working slider. Names of outputs are data from the driver, not interface text.

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "audio";

/// `audio.state` – the output in use and its volume (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct AudioState {
    /// A sound card with playback controls exists.
    pub present: bool,
    /// Volume of the output in use, 0–100.
    pub volume_percent: u8,
    /// Muted.
    pub muted: bool,
    /// Name of the output in use (empty without device).
    pub output_name: String,
    /// Id of the output in use.
    pub output_id: Option<String>,
    /// The output has a volume control (some have only a switch).
    pub has_volume: bool,
}

/// One output.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Output {
    /// Stable id.
    pub id: String,
    /// Name from the driver.
    pub name: String,
    /// In use.
    pub active: bool,
}

/// `audio.set_volume` – volume of the output in use.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetVolume {
    /// 0–100 (larger values are refused).
    pub percent: u8,
}

/// `audio.set_mute` – mute or unmute the output in use.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetMute {
    /// Muted.
    pub on: bool,
}

/// `audio.outputs` – the outputs of the device.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Outputs {}

/// Answer to [`Outputs`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct OutputsReply {
    /// The outputs.
    pub outputs: Vec<Output>,
}

/// `audio.set_output` – use another output.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetOutput {
    /// Id from [`Outputs`].
    pub id: String,
}

/// How a change went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioOutcome {
    /// Done.
    Done,
    /// There is no sound device.
    NoDevice,
    /// No such output, or a value out of range.
    Invalid,
    /// The output has no such control.
    Unsupported,
    /// The driver refused.
    Failed,
}

/// Answer to the changes.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct AudioReply {
    /// The result.
    pub outcome: AudioOutcome,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    AudioState,
    Output,
    SetVolume,
    SetMute,
    Outputs,
    OutputsReply,
    SetOutput,
    AudioReply
);

impl Topic for AudioState {
    const TOPIC: &'static str = "audio.state";
    const RETAINED: bool = true;
}

macro_rules! call {
    ($t:ty, $m:literal, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
call!(SetVolume, "set_volume", AudioReply);
call!(SetMute, "set_mute", AudioReply);
call!(Outputs, "outputs", OutputsReply);
call!(SetOutput, "set_output", AudioReply);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn messages_round_trip() {
        let s = AudioState {
            present: true,
            volume_percent: 40,
            muted: false,
            output_name: "HDA Intel PCH".into(),
            output_id: Some("card0".into()),
            has_volume: true,
        };
        assert_eq!(decode::<AudioState>(&encode(&s).unwrap()).unwrap(), s);
        let o = OutputsReply {
            outputs: vec![Output {
                id: "card0".into(),
                name: "x".into(),
                active: true,
            }],
        };
        assert_eq!(decode::<OutputsReply>(&encode(&o).unwrap()).unwrap(), o);
        let r = AudioReply {
            outcome: AudioOutcome::NoDevice,
        };
        assert_eq!(decode::<AudioReply>(&encode(&r).unwrap()).unwrap(), r);
    }
}
