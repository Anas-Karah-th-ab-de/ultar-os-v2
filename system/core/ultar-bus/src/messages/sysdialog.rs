//! Contract of the system dialog service `ultar-sysdialog`: questions only
//! the system may ask, shown in the compositor's trusted frame (lock mark,
//! everything else dimmed) – never by the app that wants something.
//!
//! The answer comes when the user decided. A question nobody answers is
//! answered for the user after [`ANSWER_WITHIN_MS`] with the safe choice
//! (deny, cancel) – before the broker forgets the request (60 s). Callers
//! wait at least that long ([`CALL_TIMEOUT_MS`]).
//!
//! The "keep these display settings?" question has no call: the service
//! follows `settings.pending` and answers `settings.confirm` /
//! `settings.revert` itself.

use crate::message::{Call, Message, Text};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "sysdialog";
/// A question is answered for the user (deny / cancel) after this time.
pub const ANSWER_WITHIN_MS: u64 = 55_000;
/// How long a caller waits for the answer.
pub const CALL_TIMEOUT_MS: u64 = 58_000;
/// Questions waiting at once (the rest is answered "deny" / "cancel" at once).
pub const MAX_QUEUED: usize = 8;
/// Longest app id and app name in bytes.
pub const MAX_APP_NAME: usize = 128;

/// What an app asks for (DESKTOP-SPEC §5.3 "Berechtigungsfrage", §8 Apps).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Permission {
    /// The camera.
    Camera,
    /// The microphone.
    Microphone,
    /// The location.
    Location,
    /// Sharing the screen.
    ScreenShare,
    /// Files outside the app's own area.
    Files,
    /// The network.
    Network,
    /// Running in the background.
    Background,
    /// Showing notifications.
    Notifications,
}

impl Permission {
    /// Every permission.
    pub const ALL: [Permission; 8] = [
        Permission::Camera,
        Permission::Microphone,
        Permission::Location,
        Permission::ScreenShare,
        Permission::Files,
        Permission::Network,
        Permission::Background,
        Permission::Notifications,
    ];

    /// Stable lower-case word (i18n keys, settings keys).
    pub const fn word(self) -> &'static str {
        match self {
            Permission::Camera => "camera",
            Permission::Microphone => "microphone",
            Permission::Location => "location",
            Permission::ScreenShare => "screen_share",
            Permission::Files => "files",
            Permission::Network => "network",
            Permission::Background => "background",
            Permission::Notifications => "notifications",
        }
    }
}

/// `sysdialog.permission` – asks the user whether an app may use something.
/// The caller (the app launcher, a system service) is responsible for the
/// app identity: the dialog names `app_name` as given.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AskPermission {
    /// Application id (`org.ultar.files`).
    pub app_id: String,
    /// Name shown in the trusted frame (data, as the app's manifest names it).
    pub app_name: String,
    /// What it wants.
    pub permission: Permission,
    /// Why, in the app's words from its manifest (optional interface text).
    pub reason: Option<Text>,
}

/// The user's decision.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// This time only.
    AllowOnce,
    /// From now on (the caller stores it).
    AllowAlways,
    /// No (also: no answer in time, the dialog could not be shown).
    Deny,
}

/// Answer to [`AskPermission`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PermissionReply {
    /// The decision.
    pub decision: Decision,
}

/// `sysdialog.authenticate` – asks an administrator's password before an
/// administrative action. The dialog names the action and the app; the
/// password goes from the dialog to `auth.verify`, never to the caller.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Authenticate {
    /// Application id of the requester.
    pub app_id: String,
    /// Name shown in the trusted frame.
    pub app_name: String,
    /// What will be done (interface text).
    pub action: Text,
}

/// How the authentication ended.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthResult {
    /// An administrator typed the right password.
    Authorized,
    /// The user cancelled, or nobody answered in time.
    Cancelled,
    /// The account store is not usable (nobody can be authorised).
    Unavailable,
}

/// Answer to [`Authenticate`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuthenticateReply {
    /// The result.
    pub result: AuthResult,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    AskPermission,
    PermissionReply,
    Authenticate,
    AuthenticateReply
);

impl Call for AskPermission {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "permission";
    type Reply = PermissionReply;
}

impl Call for Authenticate {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "authenticate";
    type Reply = AuthenticateReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn questions_and_answers_round_trip() {
        for permission in Permission::ALL {
            let ask = AskPermission {
                app_id: "org.ultar.camera".into(),
                app_name: "الكاميرا".into(),
                permission,
                reason: Some(Text::key("app.camera.reason")),
            };
            assert_eq!(
                decode::<AskPermission>(&encode(&ask).unwrap()).unwrap(),
                ask
            );
            assert!(!permission.word().is_empty());
        }
        for decision in [Decision::AllowOnce, Decision::AllowAlways, Decision::Deny] {
            let r = PermissionReply { decision };
            assert_eq!(decode::<PermissionReply>(&encode(&r).unwrap()).unwrap(), r);
        }
        let a = Authenticate {
            app_id: "org.ultar.settings".into(),
            app_name: "Settings".into(),
            action: Text::key("settings.users.add"),
        };
        assert_eq!(decode::<Authenticate>(&encode(&a).unwrap()).unwrap(), a);
        const { assert!(ANSWER_WITHIN_MS < CALL_TIMEOUT_MS && CALL_TIMEOUT_MS < 60_000) };
    }
}
