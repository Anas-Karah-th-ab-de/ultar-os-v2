//! Contract of the account service (`account`) for the **call service**: the device key stays
//! in the account service; the call service gets proofs and signatures made with it, never the
//! key. The policy lets only the call service (`system.calld`) call these methods.
//!
//! * `device_signal_hello` / `device_signal_answer` – sign-in at the account server's
//!   signalling socket (the answer is made only for the server this device is registered with);
//! * `device_key` – this device's public key;
//! * `device_sign_call` – the device key's signature over `ultar-call-e2e-v1\0 ‖ transcript`
//!   (a 32-byte hash): the end-to-end handshake of a call, nothing else can be signed this way;
//! * `device_turn` – short-lived credentials of the TURN relay (random user name, no account).

use super::account_service::{Refusal, SERVICE};
use crate::message::{Call, Message};
use serde::{Deserialize, Serialize};

/// What the device key signs for a call: this label, then the 32-byte transcript hash.
pub const CALL_SIGN_LABEL: &[u8] = b"ultar-call-e2e-v1\0";

/// Longest challenge frame accepted.
pub const MAX_CHALLENGE: usize = 1024;

/// `account.device_signal_hello`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SignalHello {}

/// `account.device_signal_answer`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SignalAnswer {
    /// The server's challenge frame (JSON).
    pub challenge: String,
}

/// `account.device_key`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct DeviceKey {}

/// `account.device_sign_call`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SignCall {
    /// The call's transcript hash.
    pub transcript: [u8; 32],
}

/// `account.device_turn`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Turn {}

/// What the account service answers.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum DeviceAnswer {
    /// A frame for the signalling socket.
    Frame(String),
    /// The device's public key (Ed25519).
    Key([u8; 32]),
    /// A signature (64 bytes).
    Signature(Vec<u8>),
    /// TURN credentials: `host:port` of the relay, user, password.
    Turn {
        /// Address of the relay (`host:port`).
        server: String,
        /// User name.
        user: String,
        /// Password.
        password: String,
    },
}

/// Answer of every device call.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct DeviceReply {
    /// The answer, or why there is none (signed out, server away …).
    pub outcome: Result<DeviceAnswer, Refusal>,
}

impl Message for DeviceReply {
    const VERSION: u16 = 1;
}

macro_rules! calls {
    ($($call:ident => $method:literal;)*) => {$(
        impl Message for $call {
            const VERSION: u16 = 1;
        }
        impl Call for $call {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $method;
            type Reply = DeviceReply;
        }
    )*};
}

calls! {
    SignalHello => "device_signal_hello";
    SignalAnswer => "device_signal_answer";
    DeviceKey => "device_key";
    SignCall => "device_sign_call";
    Turn => "device_turn";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};
    use crate::topic::is_valid_segment_name;

    #[test]
    fn contract_names_and_round_trip() {
        for m in [
            <SignalHello as Call>::METHOD,
            <SignalAnswer as Call>::METHOD,
            <DeviceKey as Call>::METHOD,
            <SignCall as Call>::METHOD,
            <Turn as Call>::METHOD,
        ] {
            assert!(is_valid_segment_name(m), "{m}");
        }
        let r = DeviceReply {
            outcome: Ok(DeviceAnswer::Turn {
                server: "194.62.29.64:3478".into(),
                user: "1:ab".into(),
                password: "pw".into(),
            }),
        };
        assert_eq!(decode::<DeviceReply>(&encode(&r).unwrap()).unwrap(), r);
    }
}
