//! Who may change what: the bus policy decides which identities reach
//! `settings.set` at all (enforced by the broker) and which of them count
//! as administrators (asked here); the risk class of a setting decides
//! whether administrator rights are needed.

use std::sync::{Arc, Mutex, PoisonError};
use ultar_bus::{Identity, Policy};

/// The pseudo method whose call right marks an administrator: an identity
/// that the bus policy allows to call `settings.admin` may change
/// `AdminOnly` settings and report displays. The method itself does not
/// exist; calling it answers `UnknownMethod`.
pub const ADMIN_METHOD: &str = "admin";
/// Name of the settings service on the bus.
pub const SERVICE: &str = "settings";

/// Decides whether an authenticated bus identity is an administrator.
pub trait Access: Send {
    /// `true` when `identity` may change `AdminOnly` settings.
    fn is_admin(&self, identity: &str) -> bool;
}

/// Administrator rights straight from the bus policy file.
#[derive(Clone, Debug)]
pub struct PolicyAccess(pub Policy);

impl Access for PolicyAccess {
    fn is_admin(&self, identity: &str) -> bool {
        Identity::new(identity)
            .is_ok_and(|identity| self.0.may_call(&identity, SERVICE, ADMIN_METHOD))
    }
}

/// An [`Access`] that is replaced while the service runs, when the policy
/// is read again after `SIGHUP` (review round 3, R3-P1): revoking an
/// administrator needs no restart.
#[derive(Clone)]
pub struct Reloadable(Arc<Mutex<Box<dyn Access>>>);

impl Reloadable {
    /// Starts with `access`.
    pub fn new(access: Box<dyn Access>) -> Self {
        Self(Arc::new(Mutex::new(access)))
    }

    /// Applies `access` to every following decision.
    pub fn replace(&self, access: Box<dyn Access>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = access;
    }
}

impl Access for Reloadable {
    fn is_admin(&self, identity: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_admin(identity)
    }
}

/// Everybody is an administrator (development, single-user appliances).
#[derive(Clone, Copy, Debug, Default)]
pub struct AllowAll;

impl Access for AllowAll {
    fn is_admin(&self, _identity: &str) -> bool {
        true
    }
}

/// The party a request comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    /// Identity as authenticated by the bus broker.
    pub identity: String,
    /// Whether the identity has administrator rights.
    pub admin: bool,
}

impl Caller {
    /// Looks the identity's rights up.
    pub fn new(identity: &str, access: &dyn Access) -> Self {
        Self {
            identity: identity.to_owned(),
            admin: access.is_admin(identity),
        }
    }

    /// A caller with administrator rights (in-process use, tests).
    pub fn admin(identity: &str) -> Self {
        Self {
            identity: identity.to_owned(),
            admin: true,
        }
    }

    /// A caller without administrator rights.
    pub fn user(identity: &str) -> Self {
        Self {
            identity: identity.to_owned(),
            admin: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = r#"
[[rule]]
identity = "app.*"
call = ["settings.get", "settings.set"]

[[rule]]
identity = "app.settings"
call = ["settings.*"]

[[rule]]
identity = "system.displayd"
call = ["settings.admin", "settings.set_displays"]
"#;

    #[test]
    fn admin_right_comes_from_the_bus_policy() {
        let access = PolicyAccess(Policy::from_toml_str(POLICY).unwrap());
        assert!(access.is_admin("app.settings"));
        assert!(access.is_admin("system.displayd"));
        assert!(!access.is_admin("app.game"));
        assert!(!access.is_admin("stranger"));
        assert!(!access.is_admin("not a valid identity"));
        assert!(!PolicyAccess(Policy::deny_all()).is_admin("app.settings"));
        assert!(PolicyAccess(Policy::allow_all()).is_admin("anyone"));
        assert!(AllowAll.is_admin("anyone"));
    }

    #[test]
    fn caller_constructors() {
        let access = PolicyAccess(Policy::from_toml_str(POLICY).unwrap());
        assert!(Caller::new("app.settings", &access).admin);
        assert!(!Caller::new("app.game", &access).admin);
        assert!(Caller::admin("x").admin);
        assert_eq!(
            Caller::user("x"),
            Caller {
                identity: "x".into(),
                admin: false
            }
        );
    }

    #[test]
    fn a_replaced_policy_applies_to_the_next_decision() {
        let access = Reloadable::new(Box::new(AllowAll));
        let shared = access.clone();
        assert!(access.is_admin("app.settings"));
        shared.replace(Box::new(PolicyAccess(Policy::deny_all())));
        assert!(!access.is_admin("app.settings"));
    }
}
