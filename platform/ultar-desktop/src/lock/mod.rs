//! The lock screen (DESKTOP-SPEC §5.3, boards Lock and Login): first the cover – clock,
//! date with its Hijri date, notifications without their content, unlock; then the sign-in
//! of the locked user – password or PIN, show while held, Caps Lock hint, keyboard layout,
//! wrong tries wait longer (the authentication service decides the wait).
//!
//! The state machine here has no I/O: the program (`ultar-lock`) feeds it compositor and
//! service events and runs its [`LockEffect`]s. A password lives only in the toolkit's
//! password field (wiped buffer) and in the [`Secret`] of one `auth.verify` call.

pub mod access;
mod compact;
pub mod view;

use ultar_bus::messages::auth::{Method, Outcome as AuthOutcome, Secret, UserInfo};

/// Where the lock screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Not locked: no surface.
    Hidden,
    /// Locked: clock and unlock button.
    Cover,
    /// The sign-in of the locked user.
    SignIn,
    /// The password is being checked.
    Checking,
}

/// What went wrong at the sign-in (shown under the field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignInError {
    /// Wrong password or PIN.
    Wrong,
    /// The account store is damaged (nobody can sign in).
    Unavailable,
    /// The authentication service does not answer.
    NoService,
    /// No user is set up on this device.
    NoUser,
}

/// Something the lock screen asks of the world.
#[derive(Debug, PartialEq)]
pub enum LockEffect {
    /// Check a secret (`auth.verify`).
    Verify {
        /// User.
        user: String,
        /// Password or PIN.
        method: Method,
        /// The typed secret (wiped when dropped).
        secret: Secret,
    },
    /// The user is authenticated: lift the lock (compositor `Unlock`).
    Unlock,
    /// Select a keyboard layout.
    Layout(u8),
}

/// The lock screen's state.
#[derive(Debug, Clone, PartialEq)]
pub struct LockState {
    /// Stage.
    pub stage: Stage,
    /// The user that signs in.
    pub user: Option<UserInfo>,
    /// Every account that can sign in (shown as a choice when there are several).
    pub users: Vec<UserInfo>,
    /// Password or PIN.
    pub method: Method,
    /// Last error.
    pub error: Option<SignInError>,
    /// No try before this time (ms since the epoch).
    pub retry_until_ms: u64,
    /// Caps Lock is on.
    pub caps_lock: bool,
    /// Keyboard layouts and the active one.
    pub layouts: (Vec<String>, u8),
    /// Notifications per app (no content on the lock screen).
    pub notes: Vec<(String, usize)>,
    /// What the system services offer the lock screen (power off, the playing medium).
    pub svc: LockServices,
    /// login.access: the accessibility menu and its values.
    pub access: access::AccessState,
}

impl Default for LockState {
    fn default() -> LockState {
        LockState {
            stage: Stage::Hidden,
            user: None,
            users: Vec::new(),
            method: Method::Password,
            error: None,
            retry_until_ms: 0,
            caps_lock: false,
            layouts: (Vec::new(), 0),
            notes: Vec::new(),
            svc: LockServices::default(),
            access: access::AccessState::default(),
        }
    }
}

/// The services' part of the lock screen (`power.actions`, `media.state`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LockServices {
    /// The power service can switch the device off (lock.power).
    pub power_off: bool,
    /// What plays (lock.media): title, artist, playing, can go back, can go on.
    pub media: Option<(String, String, bool, bool, bool)>,
}

impl LockState {
    /// login.user: account `index` of the list signs in (its card is chosen, the sign-in
    /// shows); `false` for an index that is not in the list.
    pub fn choose_user(&mut self, index: usize) -> bool {
        let Some(user) = self.users.get(index).cloned() else {
            return false;
        };
        self.user = Some(user);
        self.error = None;
        self.show_sign_in();
        true
    }

    /// The session was locked: show the cover.
    pub fn locked(&mut self) {
        if self.stage == Stage::Hidden {
            self.stage = Stage::Cover;
            self.error = None;
        }
    }

    /// The session is unlocked: hide everything.
    pub fn unlocked(&mut self) {
        self.stage = Stage::Hidden;
        self.access.open = false;
        self.error = None;
        self.method = Method::Password;
    }

    /// lock.unlock (any key, click or swipe on the cover): show the sign-in.
    pub fn show_sign_in(&mut self) {
        if self.stage == Stage::Cover {
            self.stage = Stage::SignIn;
            if self.user.is_none() {
                self.error = Some(SignInError::NoUser);
            }
        }
    }

    /// Back to the cover (Esc on the sign-in).
    pub fn back(&mut self) {
        if self.access.open {
            self.access.open = false;
        } else if self.stage == Stage::SignIn {
            self.stage = Stage::Cover;
        }
    }

    /// login.method: password ↔ PIN (only with a PIN set up).
    pub fn switch_method(&mut self) {
        if self.user.as_ref().is_some_and(|u| u.has_pin) {
            self.method = match self.method {
                Method::Password => Method::Pin,
                Method::Pin => Method::Password,
            };
            self.error = None;
        }
    }

    /// Remaining wait before a try is accepted.
    pub fn wait_ms(&self, now_ms: u64) -> u64 {
        self.retry_until_ms.saturating_sub(now_ms)
    }

    /// login.submit / Enter: checks the typed secret – not during a wait, not empty, not
    /// twice at once. The secret is consumed either way.
    pub fn submit(&mut self, secret: Secret, now_ms: u64) -> Option<LockEffect> {
        let user = self.user.as_ref()?.name.clone();
        if self.stage != Stage::SignIn || secret.is_empty() || self.wait_ms(now_ms) > 0 {
            return None;
        }
        self.stage = Stage::Checking;
        Some(LockEffect::Verify {
            user,
            method: self.method,
            secret,
        })
    }

    /// The answer of the authentication service (`None`: it did not answer).
    pub fn verified(&mut self, outcome: Option<AuthOutcome>, now_ms: u64) -> Option<LockEffect> {
        if self.stage != Stage::Checking {
            return None;
        }
        self.stage = Stage::SignIn;
        match outcome {
            Some(AuthOutcome::Granted) => {
                self.error = None;
                self.retry_until_ms = 0;
                return Some(LockEffect::Unlock);
            }
            Some(
                AuthOutcome::Denied { retry_after_ms } | AuthOutcome::Throttled { retry_after_ms },
            ) => {
                self.error = Some(SignInError::Wrong);
                self.retry_until_ms = now_ms.saturating_add(retry_after_ms);
            }
            Some(AuthOutcome::Unavailable) => self.error = Some(SignInError::Unavailable),
            None => self.error = Some(SignInError::NoService),
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(pin: bool) -> UserInfo {
        UserInfo {
            name: "anas".into(),
            display_name: "أنس".into(),
            has_pin: pin,
            admin: true,
        }
    }

    #[test]
    fn cover_sign_in_check_and_unlock() {
        let mut s = LockState {
            user: Some(user(true)),
            ..LockState::default()
        };
        s.show_sign_in();
        assert_eq!(s.stage, Stage::Hidden, "nothing before the lock");
        s.locked();
        s.show_sign_in();
        assert_eq!(s.stage, Stage::SignIn);
        assert!(
            s.submit(Secret::default(), 0).is_none(),
            "empty is not sent"
        );
        let effect = s.submit(Secret::new("pw".into()), 0).unwrap();
        assert!(format!("{effect:?}").contains("<hidden>"));
        assert!(
            s.submit(Secret::new("pw".into()), 0).is_none(),
            "one check at a time"
        );
        let denied = AuthOutcome::Denied {
            retry_after_ms: 2000,
        };
        assert_eq!(s.verified(Some(denied), 100), None);
        assert_eq!((s.error, s.wait_ms(1100)), (Some(SignInError::Wrong), 1000));
        assert!(
            s.submit(Secret::new("pw".into()), 1500).is_none(),
            "waiting"
        );
        assert!(s.submit(Secret::new("pw".into()), 2100).is_some());
        assert_eq!(
            s.verified(Some(AuthOutcome::Granted), 2200),
            Some(LockEffect::Unlock)
        );
        s.unlocked();
        assert_eq!(s.stage, Stage::Hidden);
    }

    #[test]
    fn methods_errors_and_missing_users() {
        let mut s = LockState::default();
        s.locked();
        s.show_sign_in();
        assert_eq!(s.error, Some(SignInError::NoUser));
        assert!(s.submit(Secret::new("x".into()), 0).is_none());
        let mut p = LockState {
            user: Some(user(false)),
            ..LockState::default()
        };
        p.switch_method();
        assert_eq!(p.method, Method::Password, "no PIN set up");
        p.user = Some(user(true));
        p.switch_method();
        assert_eq!(p.method, Method::Pin);
        p.locked();
        p.show_sign_in();
        p.back();
        assert_eq!(p.stage, Stage::Cover);
        p.show_sign_in();
        p.submit(Secret::new("1234".into()), 0);
        p.verified(None, 0);
        assert_eq!(p.error, Some(SignInError::NoService));
        p.submit(Secret::new("1234".into()), 0);
        p.verified(Some(AuthOutcome::Unavailable), 0);
        assert_eq!(p.error, Some(SignInError::Unavailable));
        assert_eq!(
            p.verified(Some(AuthOutcome::Granted), 0),
            None,
            "no check pending"
        );
    }
}
