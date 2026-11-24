//! Errors of the GT logic. The kernel module logs them (technical text, not shown to users).

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
    /// The register block does not answer (all ones: device off, in reset or gone).
    DeviceGone,
    /// The device reports an IP version this driver has no description for.
    UnknownIp {
        /// Version times 100 (12.74 → 1274).
        verx100: u32,
        /// `true`: the media IP, `false`: the graphics IP.
        media: bool,
    },
    /// A power domain did not acknowledge a wake or sleep request.
    Forcewake {
        /// Acknowledge register.
        reg: u32,
        /// The value read last.
        ack: u32,
    },
    /// The firmware file is damaged or not one this driver can load.
    Firmware(&'static str),
    /// The firmware does not fit the protected memory, or its registers refuse the layout.
    Wopcm(&'static str),
    /// The GuC did not go into reset.
    GucReset {
        /// `GUC_STATUS` after the reset request.
        status: u32,
    },
    /// The GuC reported a load failure or did not become ready in time.
    GucLoad {
        /// `GUC_STATUS` read last.
        status: u32,
    },
    /// The GuC answered a request with a failure.
    GucRequest {
        /// First word of the request (the action).
        request: u32,
        /// The GuC's error code.
        error: u32,
        /// The GuC's hint to the error.
        hint: u32,
    },
    /// The GuC did not answer a request, or with something that is no response.
    GucProtocol {
        /// First word of the request (the action).
        request: u32,
        /// What the reply register held.
        reply: u32,
    },
    /// A value is outside what the hardware or this driver accepts.
    Invalid(&'static str),
    /// The request is valid but not implemented by this driver.
    NotSupported(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Timeout { reg, last } => {
                write!(f, "register {reg:#x} timed out (last {last:#010x})")
            }
            Error::DeviceGone => f.write_str("GT registers read all ones"),
            Error::UnknownIp { verx100, media } => write!(
                f,
                "unknown {} IP {}.{:02}",
                if *media { "media" } else { "graphics" },
                verx100 / 100,
                verx100 % 100
            ),
            Error::Forcewake { reg, ack } => {
                write!(f, "forcewake not acknowledged ({reg:#x} = {ack:#010x})")
            }
            Error::Firmware(w) => write!(f, "firmware: {w}"),
            Error::Wopcm(w) => write!(f, "WOPCM: {w}"),
            Error::GucReset { status } => {
                write!(f, "GuC did not enter reset (status {status:#010x})")
            }
            Error::GucLoad { status } => write!(
                f,
                "GuC load failed (status {status:#010x}: {})",
                crate::guc::Status::decode(*status)
            ),
            Error::GucRequest {
                request,
                error,
                hint,
            } => {
                write!(
                    f,
                    "GuC refused request {request:#x} (error {error:#x}, hint {hint:#x})"
                )
            }
            Error::GucProtocol { request, reply } => {
                write!(
                    f,
                    "no valid GuC reply to request {request:#x} (reply {reply:#010x})"
                )
            }
            Error::Invalid(w) => write!(f, "invalid: {w}"),
            Error::NotSupported(w) => write!(f, "not supported: {w}"),
        }
    }
}

impl core::error::Error for Error {}

impl From<ultar_intel_display::Error> for Error {
    fn from(e: ultar_intel_display::Error) -> Self {
        match e {
            ultar_intel_display::Error::Timeout { reg, last } => Error::Timeout { reg, last },
            ultar_intel_display::Error::DeviceGone => Error::DeviceGone,
            _ => Error::Invalid("register access"),
        }
    }
}

/// Result of the GT logic.
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn texts_name_the_cause() {
        assert!(Error::Timeout {
            reg: 0xc314,
            last: 1
        }
        .to_string()
        .contains("0xc314"));
        assert_eq!(
            Error::UnknownIp {
                verx100: 2004,
                media: false
            }
            .to_string(),
            "unknown graphics IP 20.04"
        );
        assert!(Error::GucLoad {
            status: 0x8000_71ec
        }
        .to_string()
        .contains("init data"));
        assert_eq!(
            Error::from(ultar_intel_display::Error::Timeout { reg: 4, last: 2 }),
            Error::Timeout { reg: 4, last: 2 }
        );
        for e in [
            Error::DeviceGone,
            Error::Forcewake { reg: 0xdfc, ack: 0 },
            Error::Firmware("x"),
            Error::Wopcm("y"),
            Error::GucReset { status: 0 },
            Error::Invalid("z"),
            Error::GucRequest {
                request: 1,
                error: 2,
                hint: 3,
            },
            Error::GucProtocol {
                request: 1,
                reply: 2,
            },
            Error::NotSupported("w"),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }
}
