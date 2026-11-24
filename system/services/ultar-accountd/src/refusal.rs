//! Why a call was refused: i18n keys (`accountd.refused.*`, catalogs in
//! `locales/`), never finished text and never a secret.

use ultar_bus::messages::account_service::Refusal;
use ultar_bus::{Text, TextArg};

/// The reasons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// No administrator confirmed in the system dialog.
    NotAuthorized,
    /// The dialog could not be shown (no dialog service).
    DialogUnavailable,
    /// Another administrator question is open.
    Busy,
    /// Too many changes in a short time.
    Throttled,
    /// The current password was wrong.
    WrongPassword,
    /// The two new passwords differ.
    PasswordsDiffer,
    /// The password store refused the password (too short, too long).
    PasswordRejected,
    /// The password store does not answer.
    CredentialsUnavailable,
    /// Not a valid account name.
    InvalidName,
    /// A name of the system.
    ReservedName,
    /// The name exists.
    NameTaken,
    /// Not a valid display name.
    InvalidDisplayName,
    /// No such account.
    UnknownUser,
    /// The last administrator.
    LastAdmin,
    /// The signed-in user.
    CurrentUser,
    /// Too many accounts.
    TooManyUsers,
    /// No free user id.
    NoFreeId,
    /// Not a PNG or JPEG picture, or too large.
    PictureInvalid,
    /// The home folder could not be made.
    HomeFailed,
    /// The account file could not be written.
    StoreFailed,
    /// The account file is damaged (an administrator must repair it).
    StoreDamaged,
    /// The Ultar account needs a server that does not exist yet.
    NoServer,
    /// A service of the account needs a signed-in account.
    NeedsAccount,
}

impl Key {
    /// Every key (catalog completeness test).
    pub const ALL: [Key; 23] = [
        Key::NotAuthorized,
        Key::DialogUnavailable,
        Key::Busy,
        Key::Throttled,
        Key::WrongPassword,
        Key::PasswordsDiffer,
        Key::PasswordRejected,
        Key::CredentialsUnavailable,
        Key::InvalidName,
        Key::ReservedName,
        Key::NameTaken,
        Key::InvalidDisplayName,
        Key::UnknownUser,
        Key::LastAdmin,
        Key::CurrentUser,
        Key::TooManyUsers,
        Key::NoFreeId,
        Key::PictureInvalid,
        Key::HomeFailed,
        Key::StoreFailed,
        Key::StoreDamaged,
        Key::NoServer,
        Key::NeedsAccount,
    ];

    /// The i18n key.
    pub const fn key(self) -> &'static str {
        match self {
            Key::NotAuthorized => "accountd.refused.not_authorized",
            Key::DialogUnavailable => "accountd.refused.dialog_unavailable",
            Key::Busy => "accountd.refused.busy",
            Key::Throttled => "accountd.refused.throttled",
            Key::WrongPassword => "accountd.refused.wrong_password",
            Key::PasswordsDiffer => "accountd.refused.passwords_differ",
            Key::PasswordRejected => "accountd.refused.password_rejected",
            Key::CredentialsUnavailable => "accountd.refused.credentials_unavailable",
            Key::InvalidName => "accountd.refused.invalid_name",
            Key::ReservedName => "accountd.refused.reserved_name",
            Key::NameTaken => "accountd.refused.name_taken",
            Key::InvalidDisplayName => "accountd.refused.invalid_display_name",
            Key::UnknownUser => "accountd.refused.unknown_user",
            Key::LastAdmin => "accountd.refused.last_admin",
            Key::CurrentUser => "accountd.refused.current_user",
            Key::TooManyUsers => "accountd.refused.too_many_users",
            Key::NoFreeId => "accountd.refused.no_free_id",
            Key::PictureInvalid => "accountd.refused.picture_invalid",
            Key::HomeFailed => "accountd.refused.home_failed",
            Key::StoreFailed => "accountd.refused.store_failed",
            Key::StoreDamaged => "accountd.refused.store_damaged",
            Key::NoServer => "accountd.link.no_server",
            Key::NeedsAccount => "accountd.link.needs_account",
        }
    }
}

/// A refusal without arguments.
pub fn refuse(key: Key) -> Refusal {
    Refusal {
        reason: Text::key(key.key()),
    }
}

/// A refusal that names the seconds to wait (`{seconds}`).
pub fn refuse_wait(key: Key, ms: u64) -> Refusal {
    let seconds = ms.div_ceil(1000).min(i64::MAX as u64) as i64;
    Refusal {
        reason: Text::key(key.key()).with(TextArg::Number(seconds)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_distinct_and_waits_round_up() {
        let mut keys: Vec<&str> = Key::ALL.iter().map(|k| k.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), Key::ALL.len());
        let r = refuse_wait(Key::WrongPassword, 1_001);
        assert_eq!(r.reason.args, vec![TextArg::Number(2)]);
    }
}

#[cfg(test)]
mod catalogs {
    use super::Key;

    const ACTIONS: [&str; 7] = [
        "create",
        "rename",
        "picture",
        "password",
        "kind",
        "auto_login",
        "remove",
    ];

    #[test]
    fn every_key_is_in_every_catalog_arabic_first() {
        for (lang, text) in [
            ("ar", include_str!("../locales/ar.ftl")),
            ("en", include_str!("../locales/en.ftl")),
            ("de", include_str!("../locales/de.ftl")),
        ] {
            let keys: Vec<&str> = text
                .lines()
                .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
                .filter_map(|l| l.split_once(" = ").map(|(k, _)| k))
                .collect();
            for k in Key::ALL {
                assert!(keys.contains(&k.key()), "{lang}: {}", k.key());
            }
            for a in ACTIONS {
                assert!(
                    keys.contains(&format!("accountd.action.{a}").as_str()),
                    "{lang}: {a}"
                );
            }
            assert_eq!(
                keys.len(),
                Key::ALL.len() + ACTIONS.len(),
                "{lang}: nothing extra"
            );
        }
    }
}
