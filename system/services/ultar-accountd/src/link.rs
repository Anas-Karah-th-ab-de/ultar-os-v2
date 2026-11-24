//! The Ultar account of this device (DESKTOP-SPEC §12.1), offline first.
//! There is no account server yet, so the link is always "signed out" and
//! every online part answers with the reason – nothing pretends to sign in.
//! The device stays fully usable without an account (§12.5).

use crate::refusal::{refuse, Key};
use ultar_bus::messages::account_service::{
    AccountState, ChangePassword, Create, Delete, DeviceRemove, DeviceSignOut, Phase, Reply,
    SetServer, SetService, SignIn, SignOut, Status,
};
use ultar_bus::{Call, Text};

/// The server the account will use (§12.1).
pub const DEFAULT_SERVER: &str = "ultar.code-craft.tech";

/// The account as this device knows it: signed out, with the reason why
/// signing in is not possible.
pub fn state() -> AccountState {
    AccountState {
        phase: Phase::SignedOut,
        server: DEFAULT_SERVER.to_owned(),
        error: Some(Text::key(Key::NoServer.key())),
        ..AccountState::default()
    }
}

/// True for the methods of the Ultar account (`account_service`).
pub fn is_link_method(method: &str) -> bool {
    [
        Status::METHOD,
        SignIn::METHOD,
        SignOut::METHOD,
        Create::METHOD,
        SetServer::METHOD,
        SetService::METHOD,
        ChangePassword::METHOD,
        Delete::METHOD,
        DeviceSignOut::METHOD,
        DeviceRemove::METHOD,
    ]
    .contains(&method)
}

/// The answer to one of them.
pub fn answer(method: &str) -> Reply {
    let outcome = match method {
        m if m == Status::METHOD || m == SignOut::METHOD => Ok(state()),
        m if m == SetService::METHOD => Err(refuse(Key::NeedsAccount)),
        _ => Err(refuse(Key::NoServer)),
    };
    Reply { outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_parts_are_refused_with_the_reason() {
        assert_eq!(answer("status").outcome, Ok(state()));
        assert_eq!(answer("sign_out").outcome, Ok(state()));
        for m in [
            "sign_in",
            "create",
            "set_server",
            "change_password",
            "delete",
            "device_sign_out",
            "device_remove",
        ] {
            assert!(is_link_method(m));
            assert_eq!(answer(m).outcome, Err(refuse(Key::NoServer)), "{m}");
        }
        assert_eq!(
            answer("set_service").outcome,
            Err(refuse(Key::NeedsAccount))
        );
        assert!(!is_link_method("user_create"));
        assert_eq!(state().phase, Phase::SignedOut);
    }
}
