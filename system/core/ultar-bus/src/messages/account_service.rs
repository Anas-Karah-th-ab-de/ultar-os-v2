//! Contract of the account service (`account`): the Ultar account of this device
//! (DESKTOP-SPEC §12.1, board *Account*). The service holds the token in the key store and
//! talks to the server; apps never see a token or the stored password.
//!
//! [`AccountState`] (retained) is published after every change; the calls answer with the
//! new state or a [`Refusal`] whose reason is an i18n [`Text`]. Passwords travel only inside
//! calls to this service (the broker forwards them to its owner only) and are never logged.

use crate::message::{Call, Message, Text, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "account";
/// Longest user name in bytes.
pub const MAX_USER: usize = 64;
/// Longest password in bytes.
pub const MAX_PASSWORD: usize = 256;
/// Longest server address in bytes.
pub const MAX_SERVER: usize = 253;
/// Most devices listed.
pub const MAX_DEVICES: usize = 32;

/// Where the account stands.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    /// No account on this device.
    #[default]
    SignedOut,
    /// A sign-in is running.
    SigningIn,
    /// Signed in; the server answered last time.
    SignedIn,
    /// Signed in, but the server cannot be reached: the last known state is shown.
    Offline,
    /// Too many wrong passwords; sign-in is refused until `locked_minutes` passed.
    Locked,
}

/// A service of the account that can be switched per device (§12.1 "Dienste des Kontos").
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum AccountService {
    /// Receive Ultar calls on this device.
    Calls,
    /// Ultar VPN.
    Vpn,
    /// Store purchases and licences.
    StorePurchases,
    /// Encrypted backup in the account.
    Backup,
    /// Mesh over the internet.
    MeshOverInternet,
}

impl AccountService {
    /// Every service in display order.
    pub const ALL: [AccountService; 5] = [
        AccountService::Calls,
        AccountService::Vpn,
        AccountService::StorePurchases,
        AccountService::Backup,
        AccountService::MeshOverInternet,
    ];
}

/// One device registered in the account.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct DeviceEntry {
    /// Stable id on the server.
    pub id: String,
    /// Name the owner gave it.
    pub name: String,
    /// Kind: `laptop`, `desktop`, `phone`, `tablet`, `server`.
    pub kind: String,
    /// It is this device.
    pub this_device: bool,
    /// Last seen, seconds since the Unix epoch (`0` = active now).
    pub last_seen: u64,
}

/// `account.state` – the account as this device knows it (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct AccountState {
    /// Where it stands.
    pub phase: Phase,
    /// User name (empty while signed out).
    pub user: String,
    /// Server address.
    pub server: String,
    /// The Ultar number `UOS-XXXX-XXXX` (empty while signed out).
    pub number: String,
    /// Services switched on for this device.
    pub services: Vec<AccountService>,
    /// Devices of the account.
    pub devices: Vec<DeviceEntry>,
    /// Why the last operation failed (wrong password, server not reachable, time wrong).
    pub error: Option<Text>,
    /// Sign-in attempts left before the lock.
    pub attempts_remaining: u32,
    /// Minutes until a locked sign-in is allowed again.
    pub locked_minutes: u32,
}

impl Message for AccountState {
    const VERSION: u16 = 1;
}

impl Topic for AccountState {
    const TOPIC: &'static str = "account.state";
    const RETAINED: bool = true;
}

/// Why the service refused a call.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Refusal {
    /// Reason for the user.
    pub reason: Text,
}

/// Answer of every changing call.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Reply {
    /// The new state, or why nothing changed.
    pub outcome: Result<AccountState, Refusal>,
}

/// `account.status` – the current state.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Status {}

/// `account.sign_in`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SignIn {
    /// User name.
    pub user: String,
    /// Password.
    pub password: String,
}

/// `account.sign_out` – this device; local data stays.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SignOut {}

/// `account.create` – a new account; signs in on success.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Create {
    /// User name.
    pub user: String,
    /// Password.
    pub password: String,
    /// Optional e-mail for recovery.
    pub email: Option<String>,
}

/// `account.set_server` – only while signed out; the service checks that it answers.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SetServer {
    /// Host name.
    pub server: String,
}

/// `account.set_service` – switch one service for this device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetService {
    /// Which.
    pub service: AccountService,
    /// On or off.
    pub on: bool,
}

/// `account.change_password`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ChangePassword {
    /// The current password.
    pub old: String,
    /// The new password.
    pub new: String,
}

/// `account.delete` – deletes the account on the server; asks for the password.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Delete {
    /// The password.
    pub password: String,
}

/// `account.device_sign_out` – signs another device out.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct DeviceSignOut {
    /// Device id.
    pub device: String,
}

/// `account.device_remove` – removes a device from the account.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct DeviceRemove {
    /// Device id.
    pub device: String,
}

macro_rules! calls {
    ($($call:ident => $method:literal;)*) => {$(
        impl Message for $call {
            const VERSION: u16 = 1;
        }
        impl Call for $call {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $method;
            type Reply = Reply;
        }
    )*};
}

impl Message for Reply {
    const VERSION: u16 = 1;
}

calls! {
    Status => "status";
    SignIn => "sign_in";
    SignOut => "sign_out";
    Create => "create";
    SetServer => "set_server";
    SetService => "set_service";
    ChangePassword => "change_password";
    Delete => "delete";
    DeviceSignOut => "device_sign_out";
    DeviceRemove => "device_remove";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};
    use crate::topic::{is_valid_name, is_valid_segment_name};

    #[test]
    fn contract_names_and_round_trip() {
        for m in [
            <Status as Call>::METHOD,
            <SignIn as Call>::METHOD,
            <SignOut as Call>::METHOD,
            <Create as Call>::METHOD,
            <SetServer as Call>::METHOD,
            <SetService as Call>::METHOD,
            <ChangePassword as Call>::METHOD,
            <Delete as Call>::METHOD,
            <DeviceSignOut as Call>::METHOD,
            <DeviceRemove as Call>::METHOD,
        ] {
            assert!(is_valid_segment_name(m), "{m}");
        }
        assert!(is_valid_name(AccountState::TOPIC));
        let state = AccountState {
            phase: Phase::SignedIn,
            user: "anas".into(),
            services: vec![AccountService::Vpn],
            ..AccountState::default()
        };
        let reply = Reply {
            outcome: Ok(state.clone()),
        };
        assert_eq!(decode::<Reply>(&encode(&reply).unwrap()).unwrap(), reply);
    }
}
