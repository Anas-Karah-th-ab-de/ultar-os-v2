//! Contract of the account service (`account`) for the **local users** of
//! this device (DESKTOP-SPEC §8 "Benutzer", §6 step 12): create, rename,
//! picture, password, administrator or standard, automatic sign-in, remove
//! with the choice what happens to the user's files.
//!
//! [`LocalUsers`] (retained, topic `account.users`) is published after every
//! change. Passwords travel only inside calls to this service, in the
//! self-wiping [`Secret`] of the authentication contract; the account
//! service hands them to `auth` (the single credential store) and never
//! keeps or logs them. Every change of another user, every change of the
//! kind, the automatic sign-in and every creation or removal needs an
//! administrator's password in the system's own dialog
//! (`sysdialog.authenticate`), which the service asks for itself – an app
//! cannot skip it.

use super::account_service::Refusal;
pub use super::auth::Secret;
use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Longest account name (letters, digits, `-`, `_`, `.`; starts with a letter).
pub const MAX_NAME: usize = 32;
/// Longest display name in bytes.
pub const MAX_DISPLAY_NAME: usize = 128;
/// Most local users.
pub const MAX_LOCAL_USERS: usize = 64;
/// Largest own picture in bytes (PNG or JPEG).
pub const MAX_PICTURE_BYTES: usize = 256 * 1024;
/// Number of built-in pictures (`Picture::Builtin(0..BUILTIN_PICTURES)`).
pub const BUILTIN_PICTURES: u16 = 24;

/// Administrator or standard user.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum UserKind {
    /// May change the system (with their password in the system dialog).
    Administrator,
    /// Everyday use; system changes need an administrator's password.
    #[default]
    Standard,
}

/// A user's picture.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub enum Picture {
    /// The initials on a coloured circle.
    #[default]
    Initials,
    /// One of the built-in pictures.
    Builtin(u16),
    /// An own picture: PNG or JPEG bytes (at most [`MAX_PICTURE_BYTES`]).
    Image(Vec<u8>),
}

/// What the state says about a picture (the bytes come from [`GetPicture`]).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PictureInfo {
    /// Initials.
    #[default]
    Initials,
    /// A built-in picture.
    Builtin(u16),
    /// An own picture; the number changes with every new picture.
    Custom(u32),
}

/// One local user.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct LocalUser {
    /// Account name (fixed for the life of the account).
    pub name: String,
    /// Kernel user id.
    pub uid: u32,
    /// Name shown on the sign-in screen (data, not interface text).
    pub display_name: String,
    /// Administrator or standard.
    pub kind: UserKind,
    /// The picture.
    pub picture: PictureInfo,
    /// Signs in without a password at start.
    pub auto_login: bool,
    /// The user signed in on this seat now.
    pub current: bool,
}

/// `account.users` – the local users (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct LocalUsers {
    /// The users by name.
    pub users: Vec<LocalUser>,
    /// The credential store (`auth`) answers; without it no password can be
    /// set and no user created (fail closed).
    pub credentials_available: bool,
}

impl Message for LocalUsers {
    const VERSION: u16 = 1;
}

impl Topic for LocalUsers {
    const TOPIC: &'static str = "account.users";
    const RETAINED: bool = true;
}

/// Answer of every call that changes users.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct UsersReply {
    /// The users after the change, or why nothing changed.
    pub outcome: Result<LocalUsers, Refusal>,
}

/// `account.users_list`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ListUsers {}

/// `account.user_create` (administrator).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct CreateUser {
    /// Account name.
    pub name: String,
    /// Display name.
    pub display_name: String,
    /// Administrator or standard.
    pub kind: UserKind,
    /// The password.
    pub password: Secret,
    /// The password typed again.
    pub confirm: Secret,
}

/// `account.user_rename` – the display name (own: no dialog; others: administrator).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct RenameUser {
    /// Account name.
    pub user: String,
    /// New display name.
    pub display_name: String,
}

/// `account.user_picture` – sets the picture (own: no dialog; others: administrator).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SetPicture {
    /// Account name.
    pub user: String,
    /// The picture.
    pub picture: Picture,
}

/// `account.user_password` – own password with the current one; another
/// user's (or without the current one) after the administrator dialog.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SetPassword {
    /// Account name.
    pub user: String,
    /// The current password (own account).
    pub current: Option<Secret>,
    /// The new password.
    pub new: Secret,
    /// The new password typed again.
    pub confirm: Secret,
}

/// `account.user_kind` (administrator; the last administrator stays one).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SetKind {
    /// Account name.
    pub user: String,
    /// The new kind.
    pub kind: UserKind,
}

/// `account.user_auto_login` (administrator): `None` switches it off.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SetAutoLogin {
    /// The user that signs in by itself.
    pub user: Option<String>,
}

/// What happens to the files of a removed user.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RemoveData {
    /// Kept in a folder only administrators can open.
    #[default]
    Keep,
    /// Deleted.
    Delete,
}

/// `account.user_remove` (administrator; not the signed-in user, not the last administrator).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct RemoveUser {
    /// Account name.
    pub user: String,
    /// The files.
    pub data: RemoveData,
}

/// `account.user_picture_get` – the picture of a user.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct GetPicture {
    /// Account name.
    pub user: String,
}

/// Answer to [`GetPicture`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct PictureReply {
    /// The picture (`Initials` for an unknown user).
    pub picture: Picture,
}

/// `account.session` – the sign-in screen reports who signed in on the seat
/// (`None`: nobody). Only the sign-in screen's identity may call it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SetSession {
    /// Account name.
    pub user: Option<String>,
}

macro_rules! calls {
    ($($call:ident => $method:literal, $reply:ident;)*) => {$(
        impl Message for $call {
            const VERSION: u16 = 1;
        }
        impl Call for $call {
            const SERVICE: &'static str = super::account_service::SERVICE;
            const METHOD: &'static str = $method;
            type Reply = $reply;
        }
    )*};
}

impl Message for UsersReply {
    const VERSION: u16 = 1;
}

impl Message for PictureReply {
    const VERSION: u16 = 1;
}

calls! {
    ListUsers => "users_list", UsersReply;
    CreateUser => "user_create", UsersReply;
    RenameUser => "user_rename", UsersReply;
    SetPicture => "user_picture", UsersReply;
    SetPassword => "user_password", UsersReply;
    SetKind => "user_kind", UsersReply;
    SetAutoLogin => "user_auto_login", UsersReply;
    RemoveUser => "user_remove", UsersReply;
    GetPicture => "user_picture_get", PictureReply;
    SetSession => "session", UsersReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};
    use crate::topic::is_valid_segment_name;

    #[test]
    fn names_round_trips_and_hidden_passwords() {
        for m in [
            <ListUsers as Call>::METHOD,
            <CreateUser as Call>::METHOD,
            <RenameUser as Call>::METHOD,
            <SetPicture as Call>::METHOD,
            <SetPassword as Call>::METHOD,
            <SetKind as Call>::METHOD,
            <SetAutoLogin as Call>::METHOD,
            <RemoveUser as Call>::METHOD,
            <GetPicture as Call>::METHOD,
            <SetSession as Call>::METHOD,
        ] {
            assert!(is_valid_segment_name(m), "{m}");
        }
        let create = CreateUser {
            name: "leila".into(),
            display_name: "ليلى".into(),
            kind: UserKind::Administrator,
            password: Secret::new("sesam-öffne-dich".into()),
            confirm: Secret::new("sesam-öffne-dich".into()),
        };
        assert!(!format!("{create:?}").contains("sesam"));
        assert_eq!(
            decode::<CreateUser>(&encode(&create).unwrap()).unwrap(),
            create
        );
        let users = LocalUsers {
            users: vec![LocalUser {
                name: "anas".into(),
                uid: 1000,
                display_name: "أنس".into(),
                kind: UserKind::Administrator,
                picture: PictureInfo::Custom(3),
                auto_login: false,
                current: true,
            }],
            credentials_available: true,
        };
        let reply = UsersReply { outcome: Ok(users) };
        assert_eq!(
            decode::<UsersReply>(&encode(&reply).unwrap()).unwrap(),
            reply
        );
        const { assert!(LocalUsers::RETAINED) };
    }
}
