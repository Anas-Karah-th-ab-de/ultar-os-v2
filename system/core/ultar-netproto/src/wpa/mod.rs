//! WPA2-Personal as a supplicant (IEEE 802.11-2016 clause 12): the keys,
//! the EAPOL-Key frames, the RSN element and the 4-way and group-key
//! handshake. WPA3-Personal (SAE) is not done here: there is no reviewed
//! pure-Rust SAE implementation, so SAE-only networks are reported as not
//! joinable ([`crate::ie::Security::Wpa3Personal`]).

pub mod eapol;
pub mod handshake;
pub mod keys;
pub mod rsn;

pub use handshake::{GroupKey, Step, Supplicant};
pub use keys::{Pmk, Ptk};

/// Why a passphrase, element or frame was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WpaError {
    /// A passphrase must be 8–63 printable ASCII characters or 64 hex digits.
    BadPassphrase,
    /// The SSID is empty or longer than 32 bytes.
    BadSsid,
    /// A malformed RSN element.
    BadElement,
    /// A malformed EAPOL-Key frame.
    BadFrame,
    /// A frame that does not belong to this point of the handshake.
    Unexpected,
    /// The replay counter did not increase.
    Replay,
    /// The MIC is wrong (another passphrase, or a forged frame).
    BadMic,
    /// Message 3 carries another ANonce than message 1.
    NonceMismatch,
    /// The RSN element in message 3 differs from the network's (downgrade).
    RsnMismatch,
    /// The key data could not be unwrapped or holds no usable group key.
    BadKeyData,
}

impl std::fmt::Display for WpaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            WpaError::BadPassphrase => "bad passphrase",
            WpaError::BadSsid => "bad ssid",
            WpaError::BadElement => "bad rsn element",
            WpaError::BadFrame => "bad eapol-key frame",
            WpaError::Unexpected => "unexpected frame",
            WpaError::Replay => "replayed frame",
            WpaError::BadMic => "bad mic",
            WpaError::NonceMismatch => "anonce changed",
            WpaError::RsnMismatch => "rsn element mismatch",
            WpaError::BadKeyData => "bad key data",
        };
        f.write_str(s)
    }
}

impl std::error::Error for WpaError {}
