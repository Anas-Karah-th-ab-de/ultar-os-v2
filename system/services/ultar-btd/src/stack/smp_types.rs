//! The types of the security manager (see `smp.rs`).

use super::super::hci::Addr;
use zeroize::Zeroizing;

/// IO capabilities.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Io {
    /// Shows a number, no input.
    DisplayOnly = 0,
    /// Shows a number and has yes/no (Ultar: the system dialog).
    DisplayYesNo = 1,
    /// Keyboard only.
    KeyboardOnly = 2,
    /// Nothing.
    NoIo = 3,
    /// Keyboard and display.
    KeyboardDisplay = 4,
}

/// SMP failure reasons used here.
pub mod reason {
    #![allow(missing_docs)]
    pub const PASSKEY_FAILED: u8 = 0x01;
    pub const CONFIRM_FAILED: u8 = 0x04;
    pub const NOT_SUPPORTED: u8 = 0x05;
    pub const AUTH_REQUIREMENTS: u8 = 0x03;
    pub const KEY_SIZE: u8 = 0x06;
    pub const UNSPECIFIED: u8 = 0x08;
    pub const INVALID_PARAMS: u8 = 0x0A;
    pub const DHKEY_FAILED: u8 = 0x0B;
    pub const NUMERIC_FAILED: u8 = 0x0C;
    /// Local only (never sent): the pairing timed out.
    pub const TIMEOUT: u8 = 0xF0;
}

/// What the person must do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ask {
    /// Allow pairing (Just Works) – no number.
    Consent,
    /// Do both devices show this number?
    Compare(u32),
    /// Type this number on the other device (we display).
    Show(u32),
    /// Type the number the other device shows (only a keyboard-capable side).
    Type,
}

/// The keys of a finished pairing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Keys {
    /// LTK in HCI (little-endian) order.
    pub ltk: Zeroizing<[u8; 16]>,
    /// Authenticated (MITM-protected: numeric comparison or passkey).
    pub authenticated: bool,
    /// The peer's IRK (big-endian) and identity address, if it gave them.
    pub identity: Option<(Zeroizing<[u8; 16]>, Addr)>,
}

/// What the state machine wants done.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Out {
    /// Send this SMP PDU.
    Send(Vec<u8>),
    /// Ask the person.
    Ask(Ask),
    /// Central: start encryption with this LTK (HCI order).
    Encrypt(Zeroizing<[u8; 16]>),
    /// Paired and encrypted.
    Done(Keys),
    /// Ended with this reason (sent to the peer already if it was ours).
    Failed(u8),
}
