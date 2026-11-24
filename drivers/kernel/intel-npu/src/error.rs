//! Errors of the NPU logic. The kernel module logs them (technical text, not shown to users).

use crate::hw::Reg;
use core::fmt;

/// What went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A register did not reach the expected value in time.
    Timeout {
        /// What the driver waited for.
        what: &'static str,
        /// The register.
        reg: Reg,
        /// The value read last.
        last: u32,
    },
    /// A handshake signal had the wrong level.
    Check {
        /// Which signal.
        what: &'static str,
        /// The register.
        reg: Reg,
        /// The value read.
        val: u32,
    },
    /// The register block does not answer (all ones: device off or gone).
    DeviceGone,
    /// A device id this driver does not know.
    UnknownDevice(u16),
    /// The firmware file is damaged or not one this driver can load.
    Firmware(&'static str),
    /// A value is outside what the hardware or this driver accepts.
    Invalid(&'static str),
    /// The request is valid but not implemented by this driver.
    NotSupported(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Timeout { what, reg, last } => {
                write!(f, "timed out waiting for {what} ({reg} = {last:#010x})")
            }
            Error::Check { what, reg, val } => {
                write!(f, "{what} check failed ({reg} = {val:#010x})")
            }
            Error::DeviceGone => f.write_str("NPU registers read all ones"),
            Error::UnknownDevice(d) => write!(f, "unknown NPU device {d:#06x}"),
            Error::Firmware(w) => write!(f, "firmware: {w}"),
            Error::Invalid(w) => write!(f, "invalid: {w}"),
            Error::NotSupported(w) => write!(f, "not supported: {w}"),
        }
    }
}

impl core::error::Error for Error {}

/// Result of the NPU logic.
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn texts_name_the_cause() {
        let t = Error::Timeout {
            what: "PLL lock",
            reg: Reg::b(0x40),
            last: 0,
        }
        .to_string();
        assert_eq!(
            t,
            "timed out waiting for PLL lock (buttress+0x40 = 0x00000000)"
        );
        let t = Error::Check {
            what: "TOP NOC QACCEPTN",
            reg: Reg::ip(0x164),
            val: 1,
        }
        .to_string();
        assert!(t.contains("ip+0x164"));
        for e in [
            Error::DeviceGone,
            Error::UnknownDevice(1),
            Error::Firmware("x"),
            Error::Invalid("y"),
            Error::NotSupported("z"),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }
}
