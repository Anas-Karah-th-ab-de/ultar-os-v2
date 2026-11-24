//! Account management for administrators: shown name, administrator flag, removal
//! (`auth.update_user`, `auth.remove_user`, contract `ultar_bus::messages::auth_admin`).
//!
//! Only the account service (`ultar-accountd`) calls these, after its own rules and the
//! system dialog; the broker's policy and the `auth.admin` pseudo method guard them here
//! as well. A device always keeps at least one administrator.

use crate::name::{valid_display, valid_user};
use crate::service::Auth;
use crate::store::{self, Account};
use ultar_bus::messages::auth_admin::{ManageOutcome, RemoveUser, UpdateUser};

fn admins_left(list: &[Account]) -> usize {
    list.iter().filter(|a| a.admin).count()
}

impl Auth {
    /// Changes the shown name and/or the administrator flag of an account.
    /// `Rejected` for a caller that is no administrator, an invalid name, or a change
    /// that would leave the device without an administrator.
    pub fn update_user(&mut self, request: &UpdateUser, admin: bool) -> ManageOutcome {
        if !admin || !valid_user(&request.user) {
            return ManageOutcome::Rejected;
        }
        if let Some(display) = &request.display_name {
            if !valid_display(display) {
                return ManageOutcome::Rejected;
            }
        }
        let Ok(list) = &self.accounts else {
            return ManageOutcome::Unavailable;
        };
        let mut next = list.clone();
        let Some(account) = next.iter_mut().find(|a| a.name == request.user) else {
            return ManageOutcome::Unknown;
        };
        if let Some(display) = &request.display_name {
            account.display = display.clone();
        }
        if let Some(flag) = request.admin {
            account.admin = flag;
        }
        if admins_left(&next) == 0 {
            return ManageOutcome::Rejected;
        }
        self.commit(next)
    }

    /// Removes an account with its password and PIN. The last administrator stays.
    pub fn remove_user(&mut self, request: &RemoveUser, admin: bool) -> ManageOutcome {
        if !admin || !valid_user(&request.user) {
            return ManageOutcome::Rejected;
        }
        let Ok(list) = &self.accounts else {
            return ManageOutcome::Unavailable;
        };
        if !list.iter().any(|a| a.name == request.user) {
            return ManageOutcome::Unknown;
        }
        let next: Vec<Account> = list
            .iter()
            .filter(|a| a.name != request.user)
            .cloned()
            .collect();
        if admins_left(&next) == 0 {
            return ManageOutcome::Rejected;
        }
        self.commit(next)
    }

    fn commit(&mut self, next: Vec<Account>) -> ManageOutcome {
        match store::save(&self.config.dir, &next) {
            Ok(()) => {
                self.accounts = Ok(next);
                ManageOutcome::Done
            }
            Err(_) => ManageOutcome::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::clock::ManualClock;
    use crate::hash::Cost;
    use crate::service::{Auth, AuthConfig};
    use std::sync::Arc;
    use ultar_bus::messages::auth::{Method, Secret, SetSecret};
    use ultar_bus::messages::auth_admin::{ManageOutcome, RemoveUser, UpdateUser};

    fn auth(dir: &std::path::Path) -> Auth {
        let mut auth = Auth::open(AuthConfig {
            dir: dir.to_path_buf(),
            clock: Arc::new(ManualClock::new(1_000)),
            cost: Cost::TEST,
        });
        for name in ["amal", "badr"] {
            let request = SetSecret {
                user: name.into(),
                display_name: name.into(),
                method: Method::Password,
                current: None,
                new: Secret::new("secret-1".into()),
            };
            auth.set_secret(&request, true);
        }
        auth
    }

    fn update(user: &str, admin: Option<bool>) -> UpdateUser {
        UpdateUser {
            user: user.into(),
            display_name: Some("Badr K.".into()),
            admin,
        }
    }

    #[test]
    fn only_administrators_manage_and_the_last_administrator_stays() {
        let dir = tempdir("manage");
        let mut a = auth(&dir);
        assert_eq!(
            a.update_user(&update("badr", Some(true)), false),
            ManageOutcome::Rejected
        );
        assert_eq!(
            a.update_user(&update("amal", Some(false)), true),
            ManageOutcome::Rejected
        );
        assert_eq!(
            a.update_user(&update("badr", Some(true)), true),
            ManageOutcome::Done
        );
        assert_eq!(
            a.update_user(&update("amal", Some(false)), true),
            ManageOutcome::Done
        );
        assert_eq!(
            a.update_user(&update("nobody", None), true),
            ManageOutcome::Unknown
        );
        let users = a.users();
        assert!(users
            .iter()
            .any(|u| u.name == "badr" && u.admin && u.display_name == "Badr K."));
        let remove = |user: &str| RemoveUser { user: user.into() };
        assert_eq!(
            a.remove_user(&remove("badr"), true),
            ManageOutcome::Rejected
        );
        assert_eq!(
            a.remove_user(&remove("amal"), false),
            ManageOutcome::Rejected
        );
        assert_eq!(a.remove_user(&remove("amal"), true), ManageOutcome::Done);
        assert_eq!(a.remove_user(&remove("amal"), true), ManageOutcome::Unknown);
        assert_eq!(
            a.remove_user(&remove("../x"), true),
            ManageOutcome::Rejected
        );
        // Stored: a new instance reads the same accounts.
        let again = auth(&dir);
        assert_eq!(
            again.users().len(),
            2,
            "amal is created again by the helper"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ultar-auth-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
