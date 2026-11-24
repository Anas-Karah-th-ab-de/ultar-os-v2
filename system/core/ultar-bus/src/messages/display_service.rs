//! Contract of the display service (`display`): what the Settings app needs to know about the
//! connected displays beyond the stored settings of `settingsd` – names from the displays' own
//! identification data, the modes each display offers, what it supports – and the two actions
//! of the arrangement card ("show display numbers", "look for displays").
//!
//! The owner is the process that drives the outputs (the compositor or its display service).
//! It publishes [`Outputs`] (retained) whenever the set of displays or a mode list changes,
//! and answers [`Identify`], [`Detect`], [`Apply`], [`Keep`] and [`Revert`]. Display ids are the ids `settingsd` uses in
//! `display.<id>.*` keys (`settings.set_displays` reply), so the app joins both by id.

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "display";
/// Most displays one [`Outputs`] message describes.
pub const MAX_OUTPUTS: usize = 16;
/// Most modes listed per display.
pub const MAX_MODES: usize = 64;
/// Longest display name in bytes.
pub const MAX_NAME: usize = 64;
/// Seconds the numbers of [`Identify`] stay on the displays (DESKTOP-SPEC §13 no. 39).
pub const IDENTIFY_SECONDS: u32 = 5;

/// One mode of a display.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Mode {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Refresh rate in millihertz.
    pub refresh_mhz: u32,
}

/// One connected display.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Output {
    /// The `settingsd` display id (`display.<id>.*`).
    pub id: String,
    /// Connector name (`edp-1`, `hdmi-a-1`).
    pub connector: String,
    /// Product name from the display's identification data (`Dell U2422H`); empty if none.
    pub name: String,
    /// Built into the device (laptop panel, phone screen).
    pub internal: bool,
    /// Diagonal in tenths of an inch (`156` = 15.6″); `0` when unknown.
    pub diagonal_tenths: u16,
    /// The display's preferred mode.
    pub preferred: Mode,
    /// The mode it shows now.
    pub current: Mode,
    /// Every mode it offers, largest first.
    pub modes: Vec<Mode>,
    /// It can show HDR content.
    pub hdr: bool,
    /// Its brightness can be set (backlight or DDC/CI).
    pub brightness: bool,
    /// Colour profiles the system has for it (ids; empty = automatic only).
    pub profiles: Vec<String>,
}

/// `display.outputs` – every connected display (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Outputs {
    /// The displays, in connector order.
    pub outputs: Vec<Output>,
}

impl Message for Outputs {
    const VERSION: u16 = 1;
}

impl Topic for Outputs {
    const TOPIC: &'static str = "display.outputs";
    const RETAINED: bool = true;
}

/// `display.identify` – show each display's number large on it for [`IDENTIFY_SECONDS`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Identify {
    /// Display ids in the order of their numbers (number 1 first).
    pub order: Vec<String>,
}

/// Answer to [`Identify`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct IdentifyReply {
    /// Displays that show their number.
    pub shown: u32,
}

/// `display.detect` – look for displays again (connectors re-read).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Detect {}

/// Answer to [`Detect`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DetectReply {
    /// Displays connected now.
    pub found: u32,
    /// Displays that were not known before the call.
    pub new: u32,
}

macro_rules! call {
    ($call:ident => $method:literal, $reply:ident) => {
        impl Message for $call {
            const VERSION: u16 = 1;
        }
        impl Message for $reply {
            const VERSION: u16 = 1;
        }
        impl Call for $call {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $method;
            type Reply = $reply;
        }
    };
}

/// Super+P: the four ways of using a second display (DESKTOP-SPEC §5.4, §13 no. 44).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Projection {
    /// Only this device's own display (the built-in one; without one, the first).
    ThisDeviceOnly,
    /// Every display shows the same.
    Mirror,
    /// One desktop over every display.
    Extend,
    /// Only the second display (with three or more: the first external one).
    SecondOnly,
}

impl Projection {
    /// The four choices in the order Super+P steps through them.
    pub const ALL: [Projection; 4] = [
        Projection::ThisDeviceOnly,
        Projection::Mirror,
        Projection::Extend,
        Projection::SecondOnly,
    ];
}

/// One change of `display.apply`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum Change {
    /// A Super+P choice.
    Projection(Projection),
    /// Make this display the main display (§13 no. 64; there is no "off").
    Primary(String),
    /// New positions after dragging in the arrangement (display id, x, y in logical points).
    Arrangement(Vec<(String, i32, i32)>),
    /// Resolution and refresh rate of one display at once.
    Mode {
        /// The display.
        id: String,
        /// One of its modes.
        mode: Mode,
    },
}

/// `display.apply` – a risky change: in effect at once, kept only when
/// `display.keep` follows within the confirmation time; otherwise (also when
/// the asking app or this service ends) the previous settings come back.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Apply {
    /// What to change.
    pub change: Change,
}

/// Answer to [`Apply`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ApplyReply {
    /// Settings keys now waiting for "Keep?".
    pub pending: Vec<String>,
    /// Seconds until they go back by themselves (0: nothing changed).
    pub seconds: u32,
}

/// `display.keep` – keeps the waiting display changes (or, with `more`,
/// gives the user the confirmation time once more, §13 no. 43).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Keep {
    /// Restart the time instead of keeping.
    pub more: bool,
}

/// `display.revert` – takes the waiting display changes back now.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Revert {}

/// Answer to [`Keep`] and [`Revert`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ResolveReply {
    /// Changes kept, taken back, or given more time.
    pub resolved: u32,
}

/// Service error codes (`ErrorCode::Service(n)`).
pub mod code {
    /// No display with this id is connected.
    pub const UNKNOWN_DISPLAY: u32 = 1;
    /// The mode is not one of the display's, or the change makes no sense
    /// (Super+P with one display, positions that overlap).
    pub const INVALID: u32 = 2;
    /// The settings service did not answer or refused.
    pub const SETTINGS_UNAVAILABLE: u32 = 3;
    /// Nothing waits for "Keep?".
    pub const NOT_PENDING: u32 = 4;
    /// The compositor is not connected.
    pub const NO_COMPOSITOR: u32 = 5;
}

call!(Identify => "identify", IdentifyReply);
call!(Detect => "detect", DetectReply);
call!(Apply => "apply", ApplyReply);
call!(Keep => "keep", ResolveReply);

impl Message for Revert {
    const VERSION: u16 = 1;
}

impl Call for Revert {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "revert";
    type Reply = ResolveReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn outputs_round_trip() {
        let mode = Mode {
            width: 3840,
            height: 2160,
            refresh_mhz: 60_000,
        };
        let o = Outputs {
            outputs: vec![Output {
                id: "BOE-0x0abc-0".into(),
                connector: "edp-1".into(),
                name: String::new(),
                internal: true,
                diagonal_tenths: 156,
                preferred: mode,
                current: mode,
                modes: vec![mode],
                hdr: false,
                brightness: true,
                profiles: vec![],
            }],
        };
        let bytes = encode(&o).unwrap();
        assert_eq!(decode::<Outputs>(&bytes).unwrap(), o);
        const { assert!(Outputs::RETAINED) };
        assert_eq!(<Identify as Call>::METHOD, "identify");
        assert_eq!(<Detect as Call>::SERVICE, "display");
        let apply = Apply {
            change: Change::Arrangement(vec![("DEL-a0c4-7".into(), -1920, 0)]),
        };
        assert_eq!(decode::<Apply>(&encode(&apply).unwrap()).unwrap(), apply);
        for p in Projection::ALL {
            let a = Apply {
                change: Change::Projection(p),
            };
            assert_eq!(decode::<Apply>(&encode(&a).unwrap()).unwrap(), a);
        }
        assert_eq!(<Keep as Call>::METHOD, "keep");
        assert_eq!(<Revert as Call>::METHOD, "revert");
        assert_eq!(<Apply as Call>::METHOD, "apply");
    }
}
