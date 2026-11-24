//! Output management for the display service (needs
//! [`Rights::OUTPUT_CONFIG`](super::Rights::OUTPUT_CONFIG)).
//!
//! The display service learns every connected display in detail
//! ([`OutputDetails`]: identity from the display's own data, every mode)
//! and sets the whole arrangement at once ([`OutputSetup`]). A setup can be
//! *provisional*: the compositor goes back to the previous arrangement by
//! itself when the deadline passes or when the client that set it goes
//! away before it confirmed – so a crashed display service never leaves an
//! unusable screen behind (DESKTOP-SPEC §8 "Behalten?" after 15 s).

use super::types::OutputId;
use crate::wire::{Reader, WireError, Writer};

/// Most modes listed per output.
pub const MAX_OUTPUT_MODES: usize = 64;
/// Most outputs in one setup or one identify request.
pub const MAX_SETUP_OUTPUTS: usize = 16;
/// Longest provisional time a setup may ask for (milliseconds).
pub const MAX_PROVISIONAL_MS: u32 = 120_000;
/// Longest time the numbers of `IdentifyOutputs` stay (milliseconds).
pub const MAX_IDENTIFY_MS: u32 = 10_000;
/// Longest connector name.
pub const MAX_CONNECTOR: usize = 32;
/// Longest product name from the display's data.
pub const MAX_PRODUCT_NAME: usize = 32;

/// One mode of an output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct OutputMode {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Refresh rate in millihertz.
    pub refresh_mhz: u32,
}

impl OutputMode {
    fn encode(&self, w: &mut Writer) {
        w.u32(self.width).u32(self.height).u32(self.refresh_mhz);
    }

    fn decode(r: &mut Reader<'_>) -> Result<OutputMode, WireError> {
        Ok(OutputMode {
            width: r.u32()?,
            height: r.u32()?,
            refresh_mhz: r.u32()?,
        })
    }
}

/// How the picture is turned on an output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Transform {
    /// Landscape as built.
    #[default]
    Normal,
    /// Turned a quarter clockwise (portrait).
    Rotate90,
    /// Upside down.
    Rotate180,
    /// Turned a quarter anticlockwise (portrait, flipped).
    Rotate270,
}

impl Transform {
    fn to_u8(self) -> u8 {
        match self {
            Transform::Normal => 0,
            Transform::Rotate90 => 1,
            Transform::Rotate180 => 2,
            Transform::Rotate270 => 3,
        }
    }

    fn from_u8(v: u8) -> Result<Transform, WireError> {
        Ok(match v {
            0 => Transform::Normal,
            1 => Transform::Rotate90,
            2 => Transform::Rotate180,
            3 => Transform::Rotate270,
            _ => return Err(WireError::BadValue),
        })
    }
}

/// How several outputs are used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Arrangement {
    /// One desktop over every included output.
    #[default]
    Extend,
    /// Every included output shows what the primary shows.
    Mirror,
    /// Only the primary output is on.
    Single,
}

impl Arrangement {
    fn to_u8(self) -> u8 {
        match self {
            Arrangement::Extend => 0,
            Arrangement::Mirror => 1,
            Arrangement::Single => 2,
        }
    }

    fn from_u8(v: u8) -> Result<Arrangement, WireError> {
        Ok(match v {
            0 => Arrangement::Extend,
            1 => Arrangement::Mirror,
            2 => Arrangement::Single,
            _ => return Err(WireError::BadValue),
        })
    }
}

/// What a display says about itself (EDID): the parts that identify it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct OutputIdentity {
    /// Three-letter manufacturer code (`DEL`); empty when the display has no data.
    pub manufacturer: String,
    /// Product code.
    pub product: u16,
    /// Serial number (0 when none).
    pub serial: u32,
    /// Product name (`DELL U2422H`); empty when none.
    pub name: String,
}

/// Everything the display service knows about one connected output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputDetails {
    /// Id of the output.
    pub id: OutputId,
    /// Connector name (`eDP-1`).
    pub connector: String,
    /// Identity from the display's data.
    pub identity: OutputIdentity,
    /// Built into the device.
    pub internal: bool,
    /// Physical size in millimetres (0 = unknown).
    pub size_mm: (u32, u32),
    /// The display's preferred mode.
    pub preferred: OutputMode,
    /// The mode it runs now.
    pub current: OutputMode,
    /// Every mode it offers (at most [`MAX_OUTPUT_MODES`]).
    pub modes: Vec<OutputMode>,
}

impl OutputDetails {
    pub(crate) fn encode(&self, w: &mut Writer) {
        w.u32(self.id.0).str(&self.connector, MAX_CONNECTOR);
        w.str(&self.identity.manufacturer, 3)
            .u16(self.identity.product)
            .u32(self.identity.serial)
            .str(&self.identity.name, MAX_PRODUCT_NAME);
        w.bool(self.internal)
            .u32(self.size_mm.0)
            .u32(self.size_mm.1);
        self.preferred.encode(w);
        self.current.encode(w);
        let n = self.modes.len().min(MAX_OUTPUT_MODES);
        w.u8(n as u8);
        self.modes[..n].iter().for_each(|m| m.encode(w));
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<OutputDetails, WireError> {
        let id = OutputId(r.u32()?);
        let connector = r.str(MAX_CONNECTOR)?;
        let identity = OutputIdentity {
            manufacturer: r.str(3)?,
            product: r.u16()?,
            serial: r.u32()?,
            name: r.str(MAX_PRODUCT_NAME)?,
        };
        let internal = r.bool()?;
        let size_mm = (r.u32()?, r.u32()?);
        let preferred = OutputMode::decode(r)?;
        let current = OutputMode::decode(r)?;
        let n = usize::from(r.u8()?);
        if n > MAX_OUTPUT_MODES {
            return Err(WireError::TooLong);
        }
        let modes = (0..n)
            .map(|_| OutputMode::decode(r))
            .collect::<Result<_, _>>()?;
        Ok(OutputDetails {
            id,
            connector,
            identity,
            internal,
            size_mm,
            preferred,
            current,
            modes,
        })
    }
}

/// How one output is to be used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct OutputSetup {
    /// The output.
    pub output: OutputId,
    /// Part of the arrangement (false: switched off, e.g. not in the mirror set).
    pub included: bool,
    /// The primary output (bar, new windows, sign-in screen). Exactly one included output.
    pub primary: bool,
    /// One of the output's modes.
    pub mode: OutputMode,
    /// Device pixels per logical point × 1000 (1000–3000, 250 steps).
    pub scale_milli: u32,
    /// Position of the top left corner in logical points (extend only).
    pub position: (i32, i32),
    /// Turn of the picture.
    pub transform: Transform,
}

impl OutputSetup {
    fn encode(&self, w: &mut Writer) {
        w.u32(self.output.0).bool(self.included).bool(self.primary);
        self.mode.encode(w);
        w.u32(self.scale_milli)
            .i32(self.position.0)
            .i32(self.position.1);
        w.u8(self.transform.to_u8());
    }

    fn decode(r: &mut Reader<'_>) -> Result<OutputSetup, WireError> {
        Ok(OutputSetup {
            output: OutputId(r.u32()?),
            included: r.bool()?,
            primary: r.bool()?,
            mode: OutputMode::decode(r)?,
            scale_milli: r.u32()?,
            position: (r.i32()?, r.i32()?),
            transform: Transform::from_u8(r.u8()?)?,
        })
    }
}

/// A whole arrangement: what `ConfigureOutputs` carries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Setup {
    /// Number chosen by the client; echoed in the answers.
    pub serial: u32,
    /// Extend, mirror or one display.
    pub arrangement: Arrangement,
    /// `0`: final. Otherwise the compositor returns to the previous setup
    /// after this many milliseconds unless `ConfirmOutputs` came first
    /// (at most [`MAX_PROVISIONAL_MS`]).
    pub provisional_ms: u32,
    /// One entry per connected output.
    pub outputs: Vec<OutputSetup>,
}

impl Setup {
    pub(crate) fn encode(&self, w: &mut Writer) {
        w.u32(self.serial)
            .u8(self.arrangement.to_u8())
            .u32(self.provisional_ms);
        let n = self.outputs.len().min(MAX_SETUP_OUTPUTS);
        w.u8(n as u8);
        self.outputs[..n].iter().for_each(|o| o.encode(w));
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<Setup, WireError> {
        let serial = r.u32()?;
        let arrangement = Arrangement::from_u8(r.u8()?)?;
        let provisional_ms = r.u32()?;
        if provisional_ms > MAX_PROVISIONAL_MS {
            return Err(WireError::BadValue);
        }
        let n = usize::from(r.u8()?);
        if n > MAX_SETUP_OUTPUTS {
            return Err(WireError::TooLong);
        }
        let outputs = (0..n)
            .map(|_| OutputSetup::decode(r))
            .collect::<Result<_, _>>()?;
        Ok(Setup {
            serial,
            arrangement,
            provisional_ms,
            outputs,
        })
    }
}

/// Why a setup was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetupRefusal {
    /// An output is not connected (any more), or a connected one is missing.
    UnknownOutput,
    /// The mode is not one of the output's modes.
    UnknownMode,
    /// No included output, not exactly one primary, scale or position out of range.
    Invalid,
    /// The hardware refused the mode or the turn.
    NotSupported,
    /// Another provisional setup of another client is waiting.
    Busy,
}

/// Outcome of a setup, `ConfirmOutputs` or a provisional deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetupOutcome {
    /// In effect (provisional until confirmed, if it asked for that).
    Applied,
    /// Not taken; nothing changed.
    Refused(SetupRefusal),
    /// The provisional setup is now final.
    Confirmed,
    /// The previous setup is back: the deadline passed.
    RevertedTimeout,
    /// The previous setup is back: the displays changed meanwhile.
    RevertedHotplug,
}

impl SetupOutcome {
    pub(crate) fn to_u8(self) -> u8 {
        match self {
            SetupOutcome::Applied => 0,
            SetupOutcome::Confirmed => 1,
            SetupOutcome::RevertedTimeout => 2,
            SetupOutcome::RevertedHotplug => 3,
            SetupOutcome::Refused(SetupRefusal::UnknownOutput) => 10,
            SetupOutcome::Refused(SetupRefusal::UnknownMode) => 11,
            SetupOutcome::Refused(SetupRefusal::Invalid) => 12,
            SetupOutcome::Refused(SetupRefusal::NotSupported) => 13,
            SetupOutcome::Refused(SetupRefusal::Busy) => 14,
        }
    }

    pub(crate) fn from_u8(v: u8) -> Result<SetupOutcome, WireError> {
        Ok(match v {
            0 => SetupOutcome::Applied,
            1 => SetupOutcome::Confirmed,
            2 => SetupOutcome::RevertedTimeout,
            3 => SetupOutcome::RevertedHotplug,
            10 => SetupOutcome::Refused(SetupRefusal::UnknownOutput),
            11 => SetupOutcome::Refused(SetupRefusal::UnknownMode),
            12 => SetupOutcome::Refused(SetupRefusal::Invalid),
            13 => SetupOutcome::Refused(SetupRefusal::NotSupported),
            14 => SetupOutcome::Refused(SetupRefusal::Busy),
            _ => return Err(WireError::BadValue),
        })
    }
}

/// Writes the numbers of `IdentifyOutputs` (output, number 1–99).
pub(crate) fn encode_numbers(w: &mut Writer, numbers: &[(OutputId, u8)]) {
    let n = numbers.len().min(MAX_SETUP_OUTPUTS);
    w.u8(n as u8);
    for (o, number) in &numbers[..n] {
        w.u32(o.0).u8(*number);
    }
}

/// Reads the numbers of `IdentifyOutputs`.
pub(crate) fn decode_numbers(r: &mut Reader<'_>) -> Result<Vec<(OutputId, u8)>, WireError> {
    let n = usize::from(r.u8()?);
    if n > MAX_SETUP_OUTPUTS {
        return Err(WireError::TooLong);
    }
    (0..n)
        .map(|_| {
            let o = OutputId(r.u32()?);
            let number = r.u8()?;
            if !(1..=99).contains(&number) {
                return Err(WireError::BadValue);
            }
            Ok((o, number))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(w: u32, h: u32) -> OutputMode {
        OutputMode {
            width: w,
            height: h,
            refresh_mhz: 60_000,
        }
    }

    #[test]
    fn details_and_setups_round_trip() {
        let d = OutputDetails {
            id: OutputId(3),
            connector: "HDMI-A-1".into(),
            identity: OutputIdentity {
                manufacturer: "DEL".into(),
                product: 0xa0c4,
                serial: 77,
                name: "DELL U2422H".into(),
            },
            internal: false,
            size_mm: (527, 296),
            preferred: mode(1920, 1080),
            current: mode(1280, 720),
            modes: vec![mode(1920, 1080), mode(1280, 720)],
        };
        let mut w = Writer::new(1);
        d.encode(&mut w);
        let frame = w.finish().unwrap();
        let mut r = Reader::new(&frame[8..]);
        assert_eq!(OutputDetails::decode(&mut r).unwrap(), d);
        let setup = Setup {
            serial: 9,
            arrangement: Arrangement::Mirror,
            provisional_ms: 18_000,
            outputs: vec![OutputSetup {
                output: OutputId(3),
                included: true,
                primary: true,
                mode: mode(1920, 1080),
                scale_milli: 1250,
                position: (-1920, 4),
                transform: Transform::Rotate270,
            }],
        };
        let mut w = Writer::new(1);
        setup.encode(&mut w);
        let frame = w.finish().unwrap();
        assert_eq!(Setup::decode(&mut Reader::new(&frame[8..])).unwrap(), setup);
    }

    #[test]
    fn hostile_bodies_are_refused() {
        let mut w = Writer::new(1);
        w.u32(1).u8(7).u32(0).u8(0);
        let f = w.finish().unwrap();
        assert!(Setup::decode(&mut Reader::new(&f[8..])).is_err());
        let mut w = Writer::new(1);
        w.u32(1).u8(0).u32(MAX_PROVISIONAL_MS + 1).u8(0);
        let f = w.finish().unwrap();
        assert!(Setup::decode(&mut Reader::new(&f[8..])).is_err());
        let mut w = Writer::new(1);
        w.u32(1).u8(0).u32(0).u8(200);
        let f = w.finish().unwrap();
        assert!(Setup::decode(&mut Reader::new(&f[8..])).is_err());
        let mut w = Writer::new(1);
        w.u8(1).u32(2).u8(0);
        let f = w.finish().unwrap();
        assert!(decode_numbers(&mut Reader::new(&f[8..])).is_err());
        assert!(Transform::from_u8(4).is_err());
        assert!(SetupOutcome::from_u8(99).is_err());
        for v in [0, 1, 2, 3, 10, 11, 12, 13, 14] {
            assert_eq!(SetupOutcome::from_u8(v).unwrap().to_u8(), v);
        }
    }
}
