//! The first real contracts of Ultar OS.
//!
//! | Name | Kind | Retained | Owner |
//! |---|---|---|---|
//! | `devices.capabilities` | topic | yes | `devd` |
//! | `devices.driver_state` | topic | no | `devd` |
//! | `devices.list` / `switch` / `rollback` / `clear_quarantine` / `rescan` / `shutdown` | calls | – | `devd` |
//! | `notify.post` | call | – | notification service |
//! | `notify.posted` | topic | no | notification service |
//! | `settings.get` / `settings.set` | calls | – | `settingsd` |
//! | `settings.changed` | topic | no | `settingsd` |
//! | `settings.reset` / `list` / `confirm` / `revert` / `set_displays` / `shutdown` | calls | – | `settingsd` |
//! | `settings.pending` / `settings.pending_resolved` | topics | no | `settingsd` |
//! | `settings.store_report` | topic | yes | `settingsd` |
//! | `power.state` | topic | yes | power service |
//! | `shell.mode` | topic | yes | shell |
//! | `log.query` / `log.stats` | calls | – | log service |
//! | `init.status` / `init.control` | calls | – | `ultar-init` |
//! | `launcher.list` / `start` / `stop` / `permissions` / `grant` / `revoke` / `install` / `uninstall` | calls | – | `ultar-launcher` |
//! | `auth.users` / `verify` / `set_secret` | calls | – | `ultar-authd` |
//! | `sysdialog.permission` / `authenticate` | calls | – | `ultar-sysdialog` |
//! | `lock.lock` | call | – | `ultar-lock` |
//!
//! Every user-visible string is a [`crate::Text`] (i18n key + arguments).

pub mod auth;
pub mod device_service;
pub mod devices;
pub mod init;
pub mod launcher;
pub mod lock;
pub mod log;
pub mod notify;
pub mod power;
pub mod settings;
mod settings_ext;
pub mod shell;
pub mod sysdialog;

#[cfg(test)]
mod tests {
    use crate::message::{Call, Topic};
    use crate::topic::{is_valid_name, is_valid_segment_name};

    fn check_topic<T: Topic>() {
        assert!(is_valid_name(T::TOPIC), "{}", T::TOPIC);
        assert!(T::VERSION >= 1);
    }

    fn check_call<C: Call>() {
        assert!(is_valid_name(C::SERVICE), "{}", C::SERVICE);
        assert!(is_valid_segment_name(C::METHOD), "{}", C::METHOD);
        assert!(C::VERSION >= 1);
    }

    #[test]
    fn all_contract_names_are_valid() {
        check_topic::<super::devices::Capabilities>();
        check_topic::<super::devices::DriverState>();
        check_topic::<super::notify::Posted>();
        check_topic::<super::settings::Changed>();
        check_topic::<super::power::PowerState>();
        check_topic::<super::shell::ShellMode>();
        check_call::<super::notify::Post>();
        check_call::<super::init::Status>();
        check_call::<super::init::Control>();
        check_call::<super::launcher::List>();
        check_call::<super::launcher::Start>();
        check_call::<super::launcher::Stop>();
        check_call::<super::launcher::Permissions>();
        check_call::<super::launcher::Grant>();
        check_call::<super::launcher::Revoke>();
        check_call::<super::launcher::Install>();
        check_call::<super::launcher::Uninstall>();
        check_call::<super::auth::Users>();
        check_call::<super::auth::Verify>();
        check_call::<super::auth::SetSecret>();
        check_call::<super::sysdialog::AskPermission>();
        check_call::<super::sysdialog::Authenticate>();
        check_call::<super::lock::Lock>();
        check_call::<super::settings::Get>();
        check_call::<super::settings::Set>();
        check_call::<super::settings::Reset>();
        check_call::<super::settings::ListRequest>();
        check_call::<super::settings::Confirm>();
        check_call::<super::settings::Revert>();
        check_call::<super::settings::SetDisplays>();
        check_call::<super::settings::Shutdown>();
        check_topic::<super::settings::PendingConfirmation>();
        check_topic::<super::settings::PendingResolved>();
        check_topic::<super::settings::StoreReport>();
    }

    fn retained<T: Topic>() -> (&'static str, bool) {
        (T::TOPIC, T::RETAINED)
    }

    #[test]
    fn retained_topics_are_the_state_topics() {
        let table = [
            retained::<super::devices::Capabilities>(),
            retained::<super::power::PowerState>(),
            retained::<super::shell::ShellMode>(),
            retained::<super::devices::DriverState>(),
            retained::<super::notify::Posted>(),
            retained::<super::settings::Changed>(),
        ];
        let expected = [
            ("devices.capabilities", true),
            ("power.state", true),
            ("shell.mode", true),
            ("devices.driver_state", false),
            ("notify.posted", false),
            ("settings.changed", false),
        ];
        assert_eq!(table, expected);
    }
}
