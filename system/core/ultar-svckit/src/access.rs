//! Who may do what beyond the broker's per-method rules.
//!
//! The broker already refuses calls the policy does not list. Some answers depend on
//! *who* calls an allowed method: an administrator's tool, a system component (shell,
//! lock screen) or an app. Services ask the same policy file for pseudo methods of
//! their own service (`<service>.admin`, `<service>.system`), so one file decides and a
//! rule that opens a call to an app never makes the app an administrator.

use ultar_bus::{Identity, Policy};

/// Pseudo method: administrators of the service.
pub const ADMIN: &str = "admin";
/// Pseudo method: system components (shell, lock screen, sign-in screen).
pub const SYSTEM: &str = "system";
/// Prefix of the identities of apps.
pub const APP_PREFIX: &str = "app.";

/// The policy as the service sees its callers.
#[derive(Debug, Clone)]
pub struct Access {
    service: &'static str,
    policy: Option<Policy>,
}

impl Access {
    /// Uses `policy` for the pseudo methods of `service`. Without a policy nobody
    /// is an administrator or system component (fail closed).
    pub fn new(service: &'static str, policy: Option<Policy>) -> Self {
        Self { service, policy }
    }

    fn may(&self, caller: &str, method: &str) -> bool {
        let Some(policy) = &self.policy else {
            return false;
        };
        Identity::new(caller).is_ok_and(|id| policy.may_call(&id, self.service, method))
    }

    /// The caller may call the pseudo method `<service>.admin`.
    pub fn is_admin(&self, caller: &str) -> bool {
        self.may(caller, ADMIN)
    }

    /// The caller may call the pseudo method `<service>.system`.
    pub fn is_system(&self, caller: &str) -> bool {
        self.may(caller, SYSTEM)
    }
}

/// The app id part of an app's identity (`app.notes` → `notes`), `None` for others.
pub fn app_name(caller: &str) -> Option<&str> {
    caller.strip_prefix(APP_PREFIX).filter(|n| !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = r#"
[[rule]]
identity = "system.shell"
call = ["power.system", "power.off"]

[[rule]]
identity = "system.admin"
call = ["power.*"]

[[rule]]
identity = "app.*"
call = ["power.off"]
"#;

    #[test]
    fn pseudo_methods_decide_and_apps_are_neither() {
        let policy = Policy::from_toml_str(POLICY).unwrap();
        let access = Access::new("power", Some(policy));
        assert!(access.is_system("system.shell"));
        assert!(!access.is_admin("system.shell"));
        assert!(access.is_admin("system.admin") && access.is_system("system.admin"));
        assert!(!access.is_admin("app.notes") && !access.is_system("app.notes"));
        assert!(!access.is_system("not an identity"));
        let none = Access::new("power", None);
        assert!(!none.is_admin("system.admin") && !none.is_system("system.shell"));
    }

    #[test]
    fn app_names() {
        assert_eq!(app_name("app.notes"), Some("notes"));
        assert_eq!(app_name("app."), None);
        assert_eq!(app_name("system.shell"), None);
    }
}
