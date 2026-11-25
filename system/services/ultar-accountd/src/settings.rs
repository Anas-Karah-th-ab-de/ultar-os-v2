//! The settings the account service follows: `users.auto_login` – the user that signs
//! in at start without a password. The device setting and the per-user flag
//! (`account.user_auto_login`) say the same thing; this keeps them consistent. The
//! setting is administrator-only in settingsd, so applying it needs no dialog here.

use ultar_bus::messages::settings::{Changed, Get, SettingValue};
use ultar_bus::{BusError, Client};

/// The setting: the user that signs in at start (empty: nobody).
pub const AUTO_LOGIN: &str = "users.auto_login";

/// How long one read may take, and how long to wait before trying again.
const WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Longest account name (as [`crate::names::valid_name`]).
const MAX_NAME: usize = 32;

/// The user named by a `users.auto_login` value: a non-empty name of at most 32 bytes;
/// anything else (wrong type, empty, over-long) means nobody – the schema default.
pub fn auto_login_user(value: &SettingValue) -> Option<String> {
    match value {
        SettingValue::Text(name) => {
            let name = name.trim();
            (!name.is_empty() && name.len() <= MAX_NAME).then(|| name.to_owned())
        }
        _ => None,
    }
}

/// Applies a setting that arrived: `users.auto_login` marks the user that signs in at
/// start (settingsd already asked the administrator; no dialog here). `true` when the
/// users changed and the service should publish them.
pub fn apply(
    accounts: &mut crate::core::Accounts,
    logger: &ultar_service::log::Logger,
    key: &str,
    value: &SettingValue,
) -> bool {
    if key != AUTO_LOGIN {
        return false;
    }
    match accounts.apply_auto_login(auto_login_user(value)) {
        Ok(()) => {
            logger.log(ultar_service::log::Level::Notice, "accountd.auto_login");
            true
        }
        Err(_) => {
            logger.log(
                ultar_service::log::Level::Warn,
                "accountd.auto_login_refused",
            );
            false
        }
    }
}

/// Reads `users.auto_login` once (again until the settings service answers – it may
/// start later), then follows `settings.changed`. Every value goes to `send`; when it
/// returns `false` (the service loop ended) or the bus closes, the thread ends.
pub fn feed(
    client: Client,
    mut send: impl FnMut(String, SettingValue) -> bool + Send + 'static,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            let get = Get {
                key: AUTO_LOGIN.to_owned(),
            };
            loop {
                // Subscribe first: a change between the read and the subscription
                // would be lost otherwise.
                let Ok(changes) = client.subscribe::<Changed>() else {
                    std::thread::sleep(WAIT);
                    continue;
                };
                let initial = loop {
                    match client.call(&get, WAIT) {
                        Ok(reply) => break reply.value,
                        Err(BusError::Closed) => return,
                        Err(_) => std::thread::sleep(WAIT),
                    }
                };
                if let Some(value) = initial {
                    if !send(AUTO_LOGIN.to_owned(), value) {
                        return;
                    }
                }
                loop {
                    match changes.recv() {
                        Ok(c) => {
                            if !send(c.key, c.value) {
                                return;
                            }
                        }
                        Err(BusError::Closed) => return,
                        Err(_) => break,
                    }
                }
            }
        })
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_means_that_user_anything_else_means_nobody() {
        assert_eq!(
            auto_login_user(&SettingValue::Text("anas".into())),
            Some("anas".to_owned())
        );
        assert_eq!(
            auto_login_user(&SettingValue::Text("  anas  ".into())),
            Some("anas".to_owned())
        );
        for bad in [
            SettingValue::Text(String::new()),
            SettingValue::Text("   ".into()),
            SettingValue::Text("x".repeat(33)),
            SettingValue::Flag(true),
            SettingValue::Integer(1000),
        ] {
            assert_eq!(auto_login_user(&bad), None, "{bad:?}");
        }
    }
}
