//! Contract of the lock screen `ultar-lock`: locking on request (the shell's
//! "Lock" button). Unlocking has no call – only the lock screen itself lifts
//! the lock in the compositor after checking the password.

use crate::message::{Call, Message};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "lock";

/// `lock.lock` – locks the session now.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Lock {}

/// Answer to [`Lock`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockReply {
    /// The lock surface is shown (the compositor locks with its first frame).
    pub locked: bool,
}

impl Message for Lock {
    const VERSION: u16 = 1;
}

impl Message for LockReply {
    const VERSION: u16 = 1;
}

impl Call for Lock {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "lock";
    type Reply = LockReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn lock_round_trips() {
        let r = LockReply { locked: true };
        assert_eq!(decode::<LockReply>(&encode(&r).unwrap()).unwrap(), r);
        assert_eq!(decode::<Lock>(&encode(&Lock {}).unwrap()).unwrap(), Lock {});
    }
}
