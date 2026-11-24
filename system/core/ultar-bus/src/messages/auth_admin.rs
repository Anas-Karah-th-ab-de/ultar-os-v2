//! Account management calls of the authentication service `ultar-authd`
//! (the single credential store), used by the account service: change the
//! shown name and the administrator flag of an account, remove an account.
//! Only identities the policy gives [`super::auth::ADMIN_METHOD`] may call
//! them (the account service), and the account service itself asks the
//! user for an administrator's password first.
//!
//! Kept apart from `auth.rs` so that contract stays as the shell work
//! defined it; `ultar-authd` answers both.

use super::auth::SERVICE;
use crate::message::{Call, Message};
use serde::{Deserialize, Serialize};

/// `auth.update_user` – the shown name and/or the administrator flag.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct UpdateUser {
    /// Account name.
    pub user: String,
    /// New shown name (at most `auth::MAX_DISPLAY_NAME` bytes), if it changes.
    pub display_name: Option<String>,
    /// New administrator flag, if it changes.
    pub admin: Option<bool>,
}

/// `auth.remove_user` – removes the account and its secrets.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct RemoveUser {
    /// Account name.
    pub user: String,
}

/// How a management call ended.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManageOutcome {
    /// Done.
    Done,
    /// No such account.
    Unknown,
    /// A bad name, or the change would leave no administrator.
    Rejected,
    /// The store is damaged or could not be written (fail closed).
    Unavailable,
}

/// Answer to [`UpdateUser`] and [`RemoveUser`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ManageReply {
    /// The outcome.
    pub outcome: ManageOutcome,
}

impl Message for UpdateUser {
    const VERSION: u16 = 1;
}

impl Message for RemoveUser {
    const VERSION: u16 = 1;
}

impl Message for ManageReply {
    const VERSION: u16 = 1;
}

impl Call for UpdateUser {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "update_user";
    type Reply = ManageReply;
}

impl Call for RemoveUser {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "remove_user";
    type Reply = ManageReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};
    use crate::messages::auth::MAX_DISPLAY_NAME;

    #[test]
    fn calls_round_trip() {
        let u = UpdateUser {
            user: "leila".into(),
            display_name: Some("ليلى".into()),
            admin: Some(false),
        };
        assert_eq!(decode::<UpdateUser>(&encode(&u).unwrap()).unwrap(), u);
        let r = ManageReply {
            outcome: ManageOutcome::Rejected,
        };
        assert_eq!(decode::<ManageReply>(&encode(&r).unwrap()).unwrap(), r);
        assert_eq!(<RemoveUser as Call>::METHOD, "remove_user");
        const { assert!(MAX_DISPLAY_NAME >= 64) };
    }
}
