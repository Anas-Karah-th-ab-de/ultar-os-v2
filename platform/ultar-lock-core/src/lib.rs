//! `ultar-lock-core` – the state of the lock screen shared by every device (area 5): what
//! the desktop lock (`ultar-lock`) and the phone lock (`ultar-mobile-lock`) decide alike –
//! the stages, who may unlock, the wait after wrong tries. Each brings its own view.
//!
//! The lock screen (DESKTOP-SPEC §5.3, boards Lock and Login): first the cover – clock,
//! date with its Hijri date, notifications without their content, unlock; then the sign-in
//! of the locked user – password or PIN, show while held, Caps Lock hint, keyboard layout,
//! wrong tries wait longer (the authentication service decides the wait).
//!
//! The state machine here has no I/O: the program (`ultar-lock`) feeds it compositor and
//! service events and runs its [`LockEffect`]s. A password lives only in the toolkit's
//! password field (wiped buffer) and in the [`Secret`] of one `auth.verify` call.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod access;
pub mod emergency;
pub mod prefs;

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
    /// The password of another account was right, but the session belongs to its owner
    /// (review security S-1: until each person has their own desktop, only the person whose
    /// session is locked lifts the lock).
    OtherUser,
    /// A standard user may sign in only in the parental time window (`users.parental`).
    OutsideHours,
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
    /// The account whose session this is (`None` until the first sign-in): only it unlocks.
    pub owner: Option<String>,
    /// The account whose secret is being checked (`Stage::Checking`; public only so that
    /// tests can build a state with `..LockState::default()`).
    pub checking: Option<String>,
    /// The settings the lock screen follows (`users.*`, `emergency.*`).
    pub prefs: prefs::LockPrefs,
    /// The emergency panel and its call.
    pub emergency: emergency::EmergencyState,
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
            owner: None,
            checking: None,
            prefs: prefs::LockPrefs::default(),
            emergency: emergency::EmergencyState::default(),
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
        if self.stage == Stage::Checking || !self.prefs.list_on_login {
            return false;
        }
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
    /// twice at once, and (while `users.parental` is on) a standard user only in the time
    /// window; `local_min` is the local time in minutes since midnight. The secret is
    /// consumed either way.
    pub fn submit(&mut self, secret: Secret, now_ms: u64, local_min: u16) -> Option<LockEffect> {
        let user = self.user.as_ref()?.clone();
        if self.stage != Stage::SignIn || secret.is_empty() || self.wait_ms(now_ms) > 0 {
            return None;
        }
        if !self.prefs.allows(local_min, user.admin) {
            self.error = Some(SignInError::OutsideHours);
            return None;
        }
        self.stage = Stage::Checking;
        self.checking = Some(user.name.clone());
        Some(LockEffect::Verify {
            user: user.name,
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
        let checked = self.checking.take();
        match outcome {
            Some(AuthOutcome::Granted) => {
                let name = checked?;
                if self.owner.as_ref().is_some_and(|owner| *owner != name) {
                    self.error = Some(SignInError::OtherUser);
                    return None;
                }
                self.owner = Some(name);
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
            s.submit(Secret::default(), 0, 720).is_none(),
            "empty is not sent"
        );
        let effect = s.submit(Secret::new("pw".into()), 0, 720).unwrap();
        assert!(format!("{effect:?}").contains("<hidden>"));
        assert!(
            s.submit(Secret::new("pw".into()), 0, 720).is_none(),
            "one check at a time"
        );
        let denied = AuthOutcome::Denied {
            retry_after_ms: 2000,
        };
        assert_eq!(s.verified(Some(denied), 100), None);
        assert_eq!((s.error, s.wait_ms(1100)), (Some(SignInError::Wrong), 1000));
        assert!(
            s.submit(Secret::new("pw".into()), 1500, 720).is_none(),
            "waiting"
        );
        assert!(s.submit(Secret::new("pw".into()), 2100, 720).is_some());
        assert_eq!(
            s.verified(Some(AuthOutcome::Granted), 2200),
            Some(LockEffect::Unlock)
        );
        s.unlocked();
        assert_eq!(s.stage, Stage::Hidden);
    }

    /// Review security S-1: another account's right password never lifts the lock of a
    /// session, and the account cannot be switched while a check runs.
    #[test]
    fn only_the_sessions_owner_unlocks() {
        let other = UserInfo {
            name: "guest".into(),
            display_name: "Guest".into(),
            has_pin: false,
            admin: false,
        };
        let mut s = LockState {
            user: Some(user(false)),
            users: vec![user(false), other],
            owner: Some("anas".into()),
            ..LockState::default()
        };
        s.locked();
        assert!(s.choose_user(1));
        assert!(s.submit(Secret::new("guest-pw".into()), 0, 720).is_some());
        assert_eq!(s.verified(Some(AuthOutcome::Granted), 0), None);
        assert_eq!(s.error, Some(SignInError::OtherUser));
        // switching the card while the guest's password is checked does not help either
        assert!(s.submit(Secret::new("guest-pw".into()), 0, 720).is_some());
        assert!(!s.choose_user(0), "no switch during a check");
        assert_eq!(s.verified(Some(AuthOutcome::Granted), 0), None);
        // the owner unlocks
        assert!(s.choose_user(0));
        assert!(s.submit(Secret::new("pw".into()), 0, 720).is_some());
        assert_eq!(
            s.verified(Some(AuthOutcome::Granted), 0),
            Some(LockEffect::Unlock)
        );
        // the first sign-in makes its account the owner
        let mut first = LockState {
            user: Some(user(false)),
            ..LockState::default()
        };
        first.locked();
        first.show_sign_in();
        first.submit(Secret::new("pw".into()), 0, 720);
        assert_eq!(
            first.verified(Some(AuthOutcome::Granted), 0),
            Some(LockEffect::Unlock)
        );
        assert_eq!(first.owner.as_deref(), Some("anas"));
    }

    #[test]
    fn methods_errors_and_missing_users() {
        let mut s = LockState::default();
        s.locked();
        s.show_sign_in();
        assert_eq!(s.error, Some(SignInError::NoUser));
        assert!(s.submit(Secret::new("x".into()), 0, 720).is_none());
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
        p.submit(Secret::new("1234".into()), 0, 720);
        p.verified(None, 0);
        assert_eq!(p.error, Some(SignInError::NoService));
        p.submit(Secret::new("1234".into()), 0, 720);
        p.verified(Some(AuthOutcome::Unavailable), 0);
        assert_eq!(p.error, Some(SignInError::Unavailable));
        assert_eq!(
            p.verified(Some(AuthOutcome::Granted), 0),
            None,
            "no check pending"
        );
    }

    /// users.parental: a standard user signs in only in the window; the message says
    /// when. Administrators are never restricted; switching the setting at runtime
    /// changes the next try.
    #[test]
    fn parental_window_refuses_standard_users_outside_it() {
        let child = UserInfo {
            name: "layla".into(),
            display_name: "ليلى".into(),
            has_pin: false,
            admin: false,
        };
        let mut s = LockState {
            user: Some(child),
            prefs: prefs::LockPrefs {
                parental: true,
                ..prefs::LockPrefs::default()
            },
            ..LockState::default()
        };
        s.locked();
        s.show_sign_in();
        // 22:00 is outside 07:00–21:00: refused with the window, no check sent.
        assert_eq!(s.submit(Secret::new("pw".into()), 0, 22 * 60), None);
        assert_eq!(s.error, Some(SignInError::OutsideHours));
        assert_eq!(s.stage, Stage::SignIn);
        // Inside the window the same secret is checked.
        assert!(s.submit(Secret::new("pw".into()), 0, 12 * 60).is_some());
        s.verified(Some(AuthOutcome::Granted), 0);
        // An administrator outside the window: checked.
        let mut admin = LockState {
            user: Some(user(false)),
            prefs: s.prefs.clone(),
            ..LockState::default()
        };
        admin.locked();
        admin.show_sign_in();
        assert!(admin.submit(Secret::new("pw".into()), 0, 23 * 60).is_some());
        // Runtime change: the restriction off, the next try goes through.
        s.unlocked();
        s.locked();
        s.show_sign_in();
        assert!(s.prefs.on_changed(
            prefs::PARENTAL,
            &ultar_bus::messages::settings::SettingValue::Flag(false)
        ));
        assert!(s.submit(Secret::new("pw".into()), 0, 22 * 60).is_some());
    }

    /// users.list_on_login off: no account is chosen from a list (the app shows only
    /// the session user's name field).
    #[test]
    fn without_the_user_list_no_card_can_be_chosen() {
        let mut s = LockState {
            user: Some(user(false)),
            users: vec![user(false)],
            prefs: prefs::LockPrefs {
                list_on_login: false,
                ..prefs::LockPrefs::default()
            },
            ..LockState::default()
        };
        s.locked();
        assert!(!s.choose_user(0));
        assert!(s.prefs.on_changed(
            prefs::LIST_ON_LOGIN,
            &ultar_bus::messages::settings::SettingValue::Flag(true)
        ));
        assert!(s.choose_user(0));
    }
}
