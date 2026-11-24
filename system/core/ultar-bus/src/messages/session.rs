//! Contract of the session service: who is signed in on the seat, who is in front,
//! signing out and switching users.
//!
//! The sign-in screen (lock screen) checks the password with the authentication
//! service and then reports the person with [`SignedIn`]; nobody else may. Switching
//! users locks the screen and shows the sign-in for the chosen person
//! ([`SwitchUser`]); signing out ends that person's session ([`SignOut`]): the launcher
//! follows [`SessionState`] and ends the person's apps.

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "session";
/// Most people signed in at the same time.
pub const MAX_SIGNED_IN: usize = 8;

/// One signed-in person.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SessionUser {
    /// Account name.
    pub name: String,
    /// Shown name (data, not interface text).
    pub display_name: String,
    /// In front (the active session).
    pub active: bool,
}

/// `session.state` – the signed-in people (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SessionState {
    /// The person in front, `None` at the sign-in screen.
    pub user: Option<String>,
    /// Everybody signed in, the active one included.
    pub signed_in: Vec<String>,
}

/// `session.users` – the signed-in people with their shown names.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Users {}

/// Answer to [`Users`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct UsersReply {
    /// Signed-in people, in sign-in order.
    pub users: Vec<SessionUser>,
}

/// `session.signed_in` – the sign-in screen checked `user`'s password (lock screen only).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SignedIn {
    /// Account name.
    pub user: String,
}

/// `session.switch_user` – lock the screen and show the sign-in for `user`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SwitchUser {
    /// Account name.
    pub user: String,
}

/// `session.sign_out` – end the session of the person in front.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SignOut {}

/// How a session call went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionOutcome {
    /// Done.
    Done,
    /// No such account.
    Unknown,
    /// The caller may not do this.
    Refused,
    /// Nobody is signed in / too many are.
    Invalid,
    /// The lock screen or the account service did not answer.
    Unavailable,
}

/// Answer to [`SignedIn`], [`SwitchUser`] and [`SignOut`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionReply {
    /// The result.
    pub outcome: SessionOutcome,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    SessionState,
    Users,
    UsersReply,
    SignedIn,
    SwitchUser,
    SignOut,
    SessionReply
);

impl Topic for SessionState {
    const TOPIC: &'static str = "session.state";
    const RETAINED: bool = true;
}

macro_rules! call {
    ($t:ty, $m:literal, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
call!(Users, "users", UsersReply);
call!(SignedIn, "signed_in", SessionReply);
call!(SwitchUser, "switch_user", SessionReply);
call!(SignOut, "sign_out", SessionReply);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn messages_round_trip() {
        let state = SessionState {
            user: Some("amal".into()),
            signed_in: vec!["amal".into(), "badr".into()],
        };
        assert_eq!(
            decode::<SessionState>(&encode(&state).unwrap()).unwrap(),
            state
        );
        let users = UsersReply {
            users: vec![SessionUser {
                name: "amal".into(),
                display_name: "أمل".into(),
                active: true,
            }],
        };
        assert_eq!(
            decode::<UsersReply>(&encode(&users).unwrap()).unwrap(),
            users
        );
        let r = SessionReply {
            outcome: SessionOutcome::Unavailable,
        };
        assert_eq!(decode::<SessionReply>(&encode(&r).unwrap()).unwrap(), r);
        assert_eq!(<SignOut as Call>::METHOD, "sign_out");
    }
}
