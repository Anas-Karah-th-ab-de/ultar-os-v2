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
//! | `settings.set_accessibility` / `settings.accessibility` | calls (only `accessibility.*`) | – | `settingsd` |
//! | `power.state` | topic | yes | power service |
//! | `shell.mode` | topic | yes | shell |
//! | `log.query` / `log.stats` | calls | – | log service |
//! | `init.status` / `init.control` | calls | – | `ultar-init` |
//! | `launcher.list` / `start` / `stop` / `permissions` / `grant` / `revoke` / `install` / `uninstall` | calls | – | `ultar-launcher` |
//! | `auth.users` / `verify` / `set_secret` | calls | – | `ultar-authd` |
//! | `sysdialog.permission` / `authenticate` | calls | – | `ultar-sysdialog` |
//! | `display.outputs` | topic | yes | display service (`ultar-displayd`) |
//! | `display.identify` / `display.detect` / `apply` / `keep` / `revert` | calls | – | display service (`ultar-displayd`) |
//! | `account.state` | topic | yes | account service |
//! | `account.status` / `sign_in` / `sign_out` / `create` / `set_server` / `set_service` / `change_password` / `delete` / `device_sign_out` / `device_remove` | calls | – | account service |
//! | `account.users` | topic | yes | account service (`ultar-accountd`) |
//! | `account.users_list` / `user_create` / `user_rename` / `user_picture` / `user_password` / `user_kind` / `user_auto_login` / `user_remove` / `user_picture_get` / `session` | calls | – | account service |
//! | `auth.update_user` / `remove_user` | calls | – | `ultar-authd` (for the account service) |
//! | `session.state` | topic | yes | session service (`ultar-sessiond`) |
//! | `session.users` / `signed_in` / `switch_user` / `sign_out` | calls | – | session service |
//! | `power.actions` | topic | yes | power service (`ultar-powerd`) |
//! | `power.suspend` / `restart` / `off` | calls | – | power service |
//! | `init.power` | call | – | `ultar-init` (for the power service) |
//! | `lock.lock` | call | – | `ultar-lock` (copy of the shell work's contract) |
//! | `notify.invoke` / `dismiss` / `list` / `take_invoked` | calls | – | notification service (`ultar-notifyd`) |
//! | `notify.dismissed` / `notify.invoked` | topics | no | notification service |
//! | `calendar.day` / `add` / `remove` | calls | – | calendar service (`ultar-calendard`) |
//! | `calendar.changed` | topic | no | calendar service |
//! | `audio.state` | topic | yes | audio service (`ultar-audiod`) |
//! | `audio.set_volume` / `set_mute` / `outputs` / `set_output` | calls | – | audio service |
//! | `media.state` | topic | yes | media service (`ultar-mediad`) |
//! | `media.command` | topic | no | media service |
//! | `media.update` / `release` / `play_pause` / `next` / `previous` | calls | – | media service |
//! | `capture.screenshot` | call | – | screenshot service (`ultar-captured`) |
//! | `net.state` | topic | yes | network service (`ultar-netd`) |
//! | `net.set_wifi` / `set_airplane` / `scan` / `connect` | calls | – | network service |
//! | `net.wifi` | topic | yes | network service (Wi-Fi connection) |
//! | `net.networks` / `join` / `disconnect` / `forget` / `known` | calls | – | network service |
//! | `vpn.state` | topic | yes | VPN service (`ultar-vpnd`) |
//! | `vpn.import` / `set` / `remove` | calls | – | VPN service |
//! | `bluetooth.state` | topic | yes | Bluetooth service (`ultar-btd`) |
//! | `bluetooth.set` / `devices` / `connect` / `disconnect` | calls | – | Bluetooth service |
//! | `volumes.state` | topic | yes | removable-drive service (`ultar-volumed`) |
//! | `volumes.eject` | call | – | removable-drive service |
//! | `call.state` | topic | yes | call service (`ultar-call`, see [`call`]) |
//! | `call.dial` / `answer` / `hangup` / … | calls | – | call service |
//! | `modem.state` | topic | yes | modem service (`ultar-ril`, see [`modem`]) |
//! | `modem.dial` / `answer` / `release` / `hold` / `dtmf` / `mute` | calls | – | modem service |
//!
//! Every user-visible string is a [`crate::Text`] (i18n key + arguments).

pub mod account_local;
pub mod account_service;
pub mod audio;
pub mod auth;
pub mod auth_admin;
pub mod bluetooth;
pub mod calendar;
pub mod call;
pub mod capture;
pub mod device_service;
pub mod devices;
pub mod display_service;
pub mod init;
pub mod launcher;
pub mod lock;
pub mod log;
pub mod media;
pub mod modem;
pub mod net;
pub mod notify;
pub mod power;
pub mod session;
pub mod settings;
mod settings_access;
mod settings_ext;
pub mod shell;
pub mod sysdialog;
pub mod volumes;
pub mod vpn;

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
        check_topic::<super::call::CallState>();
        check_call::<super::call::Dial>();
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
        check_call::<super::settings::SetAccessibility>();
        check_call::<super::settings::Accessibility>();
        check_topic::<super::settings::PendingConfirmation>();
        check_topic::<super::settings::PendingResolved>();
        check_topic::<super::settings::StoreReport>();
        check_topic::<super::display_service::Outputs>();
        check_call::<super::display_service::Identify>();
        check_call::<super::display_service::Detect>();
        check_topic::<super::account_service::AccountState>();
        check_call::<super::account_service::SignIn>();
        check_call::<super::display_service::Apply>();
        check_call::<super::display_service::Keep>();
        check_call::<super::display_service::Revert>();
        check_topic::<super::account_local::LocalUsers>();
        check_call::<super::account_local::CreateUser>();
        check_call::<super::account_local::RemoveUser>();
        check_call::<super::account_local::SetSession>();
        check_call::<super::auth::Users>();
        check_call::<super::auth::Verify>();
        check_call::<super::auth::SetSecret>();
        check_call::<super::auth_admin::UpdateUser>();
        check_call::<super::auth_admin::RemoveUser>();
        check_call::<super::sysdialog::AskPermission>();
        check_call::<super::sysdialog::Authenticate>();
    }

    #[test]
    fn service_contract_names_are_valid() {
        check_topic::<super::volumes::VolumesState>();
        check_call::<super::volumes::Eject>();
        check_topic::<super::bluetooth::BluetoothState>();
        check_call::<super::bluetooth::Set>();
        check_call::<super::bluetooth::Devices>();
        check_call::<super::bluetooth::Connect>();
        check_call::<super::bluetooth::Disconnect>();
        check_call::<super::bluetooth::Scan>();
        check_call::<super::bluetooth::Pair>();
        check_call::<super::bluetooth::Forget>();
        check_call::<super::bluetooth::PairConfirm>();
        check_topic::<super::net::NetState>();
        check_call::<super::net::SetWifi>();
        check_call::<super::net::SetAirplane>();
        check_call::<super::net::Scan>();
        check_call::<super::net::Connect>();
        check_call::<super::capture::Screenshot>();
        check_topic::<super::media::MediaState>();
        check_topic::<super::media::Command>();
        check_call::<super::media::Update>();
        check_call::<super::media::Release>();
        check_call::<super::media::PlayPause>();
        check_call::<super::media::Next>();
        check_call::<super::media::Previous>();
        check_topic::<super::audio::AudioState>();
        check_call::<super::audio::SetVolume>();
        check_call::<super::audio::SetMute>();
        check_call::<super::audio::Outputs>();
        check_call::<super::audio::SetOutput>();
        check_call::<super::calendar::Day>();
        check_call::<super::calendar::Add>();
        check_call::<super::calendar::Remove>();
        check_topic::<super::calendar::Changed>();
        check_call::<super::notify::Invoke>();
        check_call::<super::notify::Dismiss>();
        check_call::<super::notify::List>();
        check_call::<super::notify::TakeInvoked>();
        check_topic::<super::notify::Dismissed>();
        check_topic::<super::notify::InvokedHint>();
        check_call::<super::lock::Lock>();
        check_topic::<super::session::SessionState>();
        check_call::<super::session::Users>();
        check_call::<super::session::SignedIn>();
        check_call::<super::session::SwitchUser>();
        check_call::<super::session::SignOut>();
        check_topic::<super::power::PowerActions>();
        check_call::<super::power::Suspend>();
        check_call::<super::power::Restart>();
        check_call::<super::power::PowerOff>();
        check_call::<super::init::Power>();
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
