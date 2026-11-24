//! Errors of the display logic. Every failure path of a sequence ends in one of these; the kernel
//! module logs them (technical text, not shown to users) and maps them to the KMS error kinds.

use core::fmt;

/// What went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A register did not reach the expected value in time.
    Timeout {
        /// Register offset in BAR0.
        reg: u32,
        /// The value read last.
        last: u32,
    },
    /// The device is not one this driver knows.
    UnknownDevice(u16),
    /// The register block does not answer (all ones: device off, in reset or gone).
    DeviceGone,
    /// No pipe is running (nothing to take over).
    NoActivePipe,
    /// The firmware's plane uses something the takeover does not support (tiling, rotation, format).
    UnsupportedPlane(u32),
    /// A value is outside what the hardware or this driver accepts (a mode, a size, a rate).
    Invalid(&'static str),
    /// The request is valid but not implemented by this driver (e.g. a mode set on display 9).
    NotSupported(&'static str),
    /// An AUX transaction failed.
    Aux(AuxError),
    /// A DDC (GMBUS) transaction failed.
    Ddc(DdcError),
    /// A PHY message-bus transaction failed.
    MsgBus {
        /// PHY register address.
        addr: u16,
        /// Status register value.
        status: u32,
    },
    /// EDID data are damaged.
    Edid(&'static str),
    /// Link training did not succeed.
    LinkTraining(&'static str),
    /// No GGTT space or no buffer memory left.
    NoSpace,
    /// The core clock is too slow for the requested pixel rate and changing it is not implemented.
    CdclkTooLow {
        /// Current core display clock (kHz).
        have: u32,
        /// Needed core display clock (kHz).
        need: u32,
    },
}

/// Why an AUX transaction failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuxError {
    /// The channel stayed busy from an earlier transaction.
    Busy,
    /// No reply from the sink (nothing attached or the sink is asleep).
    Timeout,
    /// The reply was damaged.
    Receive,
    /// The sink refused the request (NACK).
    Nack,
    /// The sink kept answering DEFER.
    Defer,
    /// The reply had an unexpected length or form.
    Protocol,
    /// The request does not fit into one AUX message (16 data bytes).
    TooLong,
}

/// Why a DDC (I2C over GMBUS) transaction failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DdcError {
    /// No acknowledge from the display (nothing attached).
    Nack,
    /// The controller did not finish in time.
    Timeout,
    /// The request is longer than the controller takes in one transfer.
    TooLong,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Timeout { reg, last } => {
                write!(f, "register {reg:#x} timed out (last {last:#010x})")
            }
            Error::UnknownDevice(d) => write!(f, "unknown display device {d:#06x}"),
            Error::DeviceGone => f.write_str("display registers read all ones"),
            Error::NoActivePipe => f.write_str("no active pipe"),
            Error::UnsupportedPlane(c) => {
                write!(f, "firmware plane not supported (PLANE_CTL {c:#010x})")
            }
            Error::Invalid(w) => write!(f, "invalid: {w}"),
            Error::NotSupported(w) => write!(f, "not supported: {w}"),
            Error::Aux(e) => write!(f, "AUX: {e:?}"),
            Error::Ddc(e) => write!(f, "DDC: {e:?}"),
            Error::MsgBus { addr, status } => write!(
                f,
                "PHY message bus {addr:#06x} failed (status {status:#010x})"
            ),
            Error::Edid(w) => write!(f, "EDID: {w}"),
            Error::LinkTraining(w) => write!(f, "link training: {w}"),
            Error::NoSpace => f.write_str("no GGTT space or buffer memory"),
            Error::CdclkTooLow { have, need } => {
                write!(f, "core clock {have} kHz below the needed {need} kHz")
            }
        }
    }
}

impl core::error::Error for Error {}

impl From<AuxError> for Error {
    fn from(e: AuxError) -> Self {
        Error::Aux(e)
    }
}

impl From<DdcError> for Error {
    fn from(e: DdcError) -> Self {
        Error::Ddc(e)
    }
}

/// Result of the display logic.
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn display_texts_name_the_cause() {
        assert!(Error::Timeout {
            reg: 0x45400,
            last: 1
        }
        .to_string()
        .contains("0x45400"));
        assert_eq!(Error::from(AuxError::Nack), Error::Aux(AuxError::Nack));
        assert_eq!(
            Error::from(DdcError::Timeout),
            Error::Ddc(DdcError::Timeout)
        );
        assert!(Error::CdclkTooLow {
            have: 192000,
            need: 300000
        }
        .to_string()
        .contains("300000"));
        for e in [
            Error::DeviceGone,
            Error::NoActivePipe,
            Error::NoSpace,
            Error::UnknownDevice(1),
            Error::Invalid("x"),
            Error::NotSupported("y"),
            Error::Edid("z"),
            Error::LinkTraining("w"),
            Error::UnsupportedPlane(5),
            Error::MsgBus { addr: 1, status: 2 },
        ] {
            assert!(!e.to_string().is_empty());
        }
    }
}
