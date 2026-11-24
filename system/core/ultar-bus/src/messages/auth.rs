//! Contract of the authentication service `ultar-authd`: the users that can
//! sign in, checking a password or PIN, and setting one.
//!
//! Who may call: the lock screen, the system dialog service and the login
//! screen (`auth.users`, `auth.verify`); `auth.set_secret` with the old secret
//! for the user's own account, without it only for an identity the policy
//! gives the pseudo method [`ADMIN_METHOD`] (the installer, Settings as
//! administrator). Secrets travel only over the local bus socket, are never
//! printed (`Debug` shows `<hidden>`) and are wiped when dropped.

use crate::message::{Call, Message};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Name of the service on the bus.
pub const SERVICE: &str = "auth";
/// Pseudo method whose call right marks an administrator of `auth`.
pub const ADMIN_METHOD: &str = "admin";
/// Longest user name (letters, digits, `-`, `_`, `.`; starts with a letter).
pub const MAX_USER_NAME: usize = 32;
/// Longest display name in bytes.
pub const MAX_DISPLAY_NAME: usize = 128;
/// Longest accepted secret in bytes.
pub const MAX_SECRET: usize = 1024;
/// Most users one reply lists.
pub const MAX_USERS: usize = 64;

/// A password or PIN on its way to the service. No `Display`, `Debug` hides
/// it, the bytes are overwritten when it is dropped.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wraps a typed secret.
    pub fn new(text: String) -> Secret {
        Secret(text)
    }

    /// The text. Whoever calls this is responsible for where it goes.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when nothing was typed.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<hidden>)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(std::mem::take(&mut self.0));
    }
}

/// Overwrites the bytes of a string before its memory is freed.
pub fn wipe(text: String) {
    let mut bytes = text.into_bytes();
    bytes.iter_mut().for_each(|b| *b = 0);
    std::hint::black_box(&bytes);
}

/// Which kind of secret.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Method {
    /// The account password.
    #[default]
    Password,
    /// A numeric PIN (4–12 digits) set up besides the password.
    Pin,
}

/// One user that can sign in.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct UserInfo {
    /// Account name.
    pub name: String,
    /// Name shown on the lock and login screens (data, not interface text).
    pub display_name: String,
    /// A PIN is set up.
    pub has_pin: bool,
    /// Administrator.
    pub admin: bool,
}

/// `auth.users` – the users that can sign in.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Users {}

/// Answer to [`Users`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct UsersReply {
    /// The users, by name.
    pub users: Vec<UserInfo>,
}

/// `auth.verify` – checks a secret.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Verify {
    /// Account name.
    pub user: String,
    /// Password or PIN.
    pub method: Method,
    /// What was typed.
    pub secret: Secret,
}

/// Result of a check.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Correct.
    Granted,
    /// Wrong (or no such user, or no PIN set up – indistinguishable); the
    /// next try is accepted after this many milliseconds.
    Denied {
        /// Wait before the next try.
        retry_after_ms: u64,
    },
    /// Not checked: tried again before the wait was over.
    Throttled {
        /// Remaining wait.
        retry_after_ms: u64,
    },
    /// The account store is unreadable or damaged: nobody can sign in until
    /// an administrator repairs it (fail closed).
    Unavailable,
}

/// Answer to [`Verify`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerifyReply {
    /// The result.
    pub outcome: Outcome,
}

/// `auth.set_secret` – sets a password or PIN.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetSecret {
    /// Account name (created when an administrator sets its password).
    pub user: String,
    /// Display name for a new account (ignored for an existing one).
    pub display_name: String,
    /// Password or PIN.
    pub method: Method,
    /// The current password; required unless the caller is an administrator.
    pub current: Option<Secret>,
    /// The new secret.
    pub new: Secret,
}

/// Why a secret was not set.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetOutcome {
    /// Stored.
    Done,
    /// The current password was wrong or missing (counts as a failed try).
    Denied {
        /// Wait before the next try.
        retry_after_ms: u64,
    },
    /// Too short, too long, a PIN with other than 4–12 digits, a bad name.
    Rejected,
    /// The store could not be written.
    Unavailable,
}

/// Answer to [`SetSecret`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetSecretReply {
    /// The result.
    pub outcome: SetOutcome,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    Users,
    UsersReply,
    Verify,
    VerifyReply,
    SetSecret,
    SetSecretReply
);

impl Call for Users {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "users";
    type Reply = UsersReply;
}

impl Call for Verify {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "verify";
    type Reply = VerifyReply;
}

impl Call for SetSecret {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "set_secret";
    type Reply = SetSecretReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn secrets_round_trip_and_never_print() {
        let v = Verify {
            user: "anas".into(),
            method: Method::Pin,
            secret: Secret::new("1234".into()),
        };
        let back = decode::<Verify>(&encode(&v).unwrap()).unwrap();
        assert_eq!(back, v);
        assert_eq!(back.secret.expose(), "1234");
        let shown = format!("{v:?}");
        assert!(
            !shown.contains("1234") && shown.contains("<hidden>"),
            "{shown}"
        );
        assert_eq!(Secret::default().len(), 0);
        assert!(Secret::default().is_empty());
    }

    #[test]
    fn outcomes_round_trip() {
        for outcome in [
            Outcome::Granted,
            Outcome::Denied {
                retry_after_ms: 2000,
            },
            Outcome::Throttled { retry_after_ms: 5 },
            Outcome::Unavailable,
        ] {
            let r = VerifyReply { outcome };
            assert_eq!(decode::<VerifyReply>(&encode(&r).unwrap()).unwrap(), r);
        }
        let s = SetSecret {
            user: "u".into(),
            display_name: "U".into(),
            method: Method::Password,
            current: Some(Secret::new("old".into())),
            new: Secret::new("new-secret".into()),
        };
        assert!(!format!("{s:?}").contains("new-secret"));
        assert_eq!(decode::<SetSecret>(&encode(&s).unwrap()).unwrap(), s);
    }
}
