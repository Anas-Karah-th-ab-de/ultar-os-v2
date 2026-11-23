//! Who may change driver choices and stop the device service.
//!
//! Reading (`devices.list`) and asking for a new search (`devices.rescan`)
//! are open to every caller the bus policy lets through. Changing which
//! driver a device uses (`switch`, `rollback`, `clear_quarantine`) and
//! stopping the service (`shutdown`) need the permission `devices.admin`:
//! a call rule for the pseudo method `admin` of the service `devices` in
//! the bus policy file. (Review round 1 found that every caller the broker
//! allowed could stop the service.)

use ultar_bus::messages::device_service::{ADMIN_METHOD, SERVICE};
use ultar_bus::{Identity, Policy};

/// Decides whether an identity has `devices.admin`.
pub trait AdminAccess: Send {
    /// `true` when `identity` may change driver choices and stop the
    /// service.
    fn is_admin(&self, identity: &str) -> bool;
}

/// Rights straight from the bus policy file. An identity that is not a
/// valid name, or has no rule, is not an administrator (default deny).
#[derive(Clone, Debug)]
pub struct PolicyAccess(pub Policy);

impl AdminAccess for PolicyAccess {
    fn is_admin(&self, identity: &str) -> bool {
        Identity::new(identity)
            .is_ok_and(|identity| self.0.may_call(&identity, SERVICE, ADMIN_METHOD))
    }
}

/// Everybody the broker lets through is an administrator (development,
/// in-process use where the embedding program decides).
#[derive(Clone, Copy, Debug, Default)]
pub struct AllowAll;

impl AdminAccess for AllowAll {
    fn is_admin(&self, _identity: &str) -> bool {
        true
    }
}

/// Methods of the service `devices` that need `devices.admin`.
pub const ADMIN_METHODS: [&str; 4] = ["switch", "rollback", "clear_quarantine", "shutdown"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_admin_rule_makes_an_administrator() {
        let policy = Policy::from_toml_str(
            r#"
[[rule]]
identity = "app.settings"
call     = ["devices.*"]

[[rule]]
identity = "app.viewer"
call     = ["devices.list", "devices.rescan", "devices.switch", "devices.shutdown"]

[[rule]]
identity = "system.update"
call     = ["devices.admin"]
"#,
        )
        .unwrap();
        let access = PolicyAccess(policy);
        assert!(access.is_admin("app.settings"));
        assert!(access.is_admin("system.update"));
        // Being allowed to *call* switch or shutdown is not the permission.
        assert!(!access.is_admin("app.viewer"));
        assert!(!access.is_admin("app.unknown"));
        assert!(!access.is_admin("Not A Name"));
        assert!(!access.is_admin(""));
        assert!(AllowAll.is_admin("anybody"));
        assert!(!PolicyAccess(Policy::deny_all()).is_admin("app.settings"));
    }
}
