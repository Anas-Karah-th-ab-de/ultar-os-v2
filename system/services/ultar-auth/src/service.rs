//! The service's logic without the bus: accounts, checks, back-off, setting secrets.

use crate::clock::Clock;
use crate::file::FileError;
use crate::hash::{self, Cost};
use crate::name::{valid_display, valid_user};
use crate::store::{self, Account};
use crate::throttle::Throttle;
use std::path::PathBuf;
use std::sync::Arc;
use ultar_bus::messages::auth::{
    Method, Outcome, Secret, SetOutcome, SetSecret, UserInfo, MAX_SECRET,
};

/// Key of the shared back-off for names that do not exist.
const UNKNOWN: &str = "*";
/// Shortest password in characters.
pub const MIN_PASSWORD: usize = 4;

/// Where and how.
#[derive(Clone)]
pub struct AuthConfig {
    /// Directory of `users` and `throttle` (owned by the service, `0700`).
    pub dir: PathBuf,
    /// Wall clock.
    pub clock: Arc<dyn Clock>,
    /// Cost of new hashes ([`Cost::STANDARD`] on devices).
    pub cost: Cost,
}

/// The accounts and the back-off.
pub struct Auth {
    config: AuthConfig,
    accounts: Result<Vec<Account>, FileError>,
    throttle: Throttle,
    /// A hash of nothing, checked for unknown users so their answer takes as long.
    dummy: String,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth")
            .field("dir", &self.config.dir)
            .field("usable", &self.accounts.is_ok())
            .finish_non_exhaustive()
    }
}

/// A PIN: 4–12 ASCII digits.
pub fn valid_pin(pin: &str) -> bool {
    (4..=12).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

fn valid_secret(method: Method, secret: &Secret) -> bool {
    match method {
        Method::Password => {
            secret.expose().chars().count() >= MIN_PASSWORD && secret.len() <= MAX_SECRET
        }
        Method::Pin => valid_pin(secret.expose()),
    }
}

impl Auth {
    /// Opens the store. Never fails: a damaged or unreadable account file leaves the
    /// service in the `Unavailable` state (nobody can sign in) – fail closed.
    pub fn open(config: AuthConfig) -> Auth {
        let now = config.clock.now_ms();
        let accounts = store::load(&config.dir);
        let throttle = Throttle::load(&config.dir, now).unwrap_or_default();
        let dummy = hash::make(b"ultar-auth-dummy", config.cost).unwrap_or_default();
        Auth {
            config,
            accounts,
            throttle,
            dummy,
        }
    }

    /// `false` when the account file is damaged or unreadable.
    pub fn usable(&self) -> bool {
        self.accounts.is_ok()
    }

    /// The users that can sign in (none while unusable).
    pub fn users(&self) -> Vec<UserInfo> {
        self.accounts
            .as_ref()
            .map(|list| list.iter().map(Account::info).collect())
            .unwrap_or_default()
    }

    fn key<'a>(&self, user: &'a str) -> &'a str {
        let known = self
            .accounts
            .as_ref()
            .is_ok_and(|list| list.iter().any(|a| a.name == user));
        if known {
            user
        } else {
            UNKNOWN
        }
    }

    /// Checks a secret. Every wrong try waits longer; tries during the wait are not
    /// checked at all.
    pub fn verify(&mut self, user: &str, method: Method, secret: &Secret) -> Outcome {
        let Ok(list) = &self.accounts else {
            return Outcome::Unavailable;
        };
        let now = self.config.clock.now_ms();
        let key = self.key(user).to_owned();
        let waiting = self
            .throttle
            .remaining(&key, now)
            .max(self.throttle.remaining(crate::throttle::EVERYONE, now));
        if waiting > 0 {
            return Outcome::Throttled {
                retry_after_ms: waiting,
            };
        }
        let stored = list
            .iter()
            .find(|a| a.name == user)
            .and_then(|a| match method {
                Method::Password => Some(a.password.clone()),
                Method::Pin => a.pin.clone(),
            });
        // Unknown user or no PIN: the same work against a dummy hash, then "wrong".
        let matched = match &stored {
            Some(phc) => hash::check(secret.expose().as_bytes(), phc),
            None => {
                let _ = hash::check(secret.expose().as_bytes(), &self.dummy);
                false
            }
        };
        if matched {
            self.throttle.succeed(&key);
            self.persist_throttle();
            return Outcome::Granted;
        }
        let wait = self.throttle.fail(&key, now);
        self.persist_throttle();
        Outcome::Denied {
            retry_after_ms: wait,
        }
    }

    fn persist_throttle(&self) {
        // A counter that cannot be stored still holds in memory; the next write retries.
        let _ = self.throttle.save(&self.config.dir);
    }

    /// Sets a password or PIN. Administrators may set any secret and create accounts;
    /// everyone else needs the current password of the account.
    pub fn set_secret(&mut self, request: &SetSecret, admin: bool) -> SetOutcome {
        if !valid_user(&request.user) || !valid_secret(request.method, &request.new) {
            return SetOutcome::Rejected;
        }
        let Ok(list) = &self.accounts else {
            return SetOutcome::Unavailable;
        };
        let exists = list.iter().any(|a| a.name == request.user);
        if !admin {
            let current = request.current.clone().unwrap_or_default();
            match self.verify(&request.user, Method::Password, &current) {
                Outcome::Granted => {}
                Outcome::Denied { retry_after_ms } | Outcome::Throttled { retry_after_ms } => {
                    return SetOutcome::Denied { retry_after_ms };
                }
                Outcome::Unavailable => return SetOutcome::Unavailable,
            }
        } else if !exists
            && (request.method == Method::Pin || !valid_display(&request.display_name))
        {
            return SetOutcome::Rejected;
        }
        let Ok(phc) = hash::make(request.new.expose().as_bytes(), self.config.cost) else {
            return SetOutcome::Rejected;
        };
        self.store_hash(request, phc)
    }

    fn store_hash(&mut self, request: &SetSecret, phc: String) -> SetOutcome {
        let Ok(list) = &self.accounts else {
            return SetOutcome::Unavailable;
        };
        let mut next = list.clone();
        match next.iter_mut().find(|a| a.name == request.user) {
            Some(a) => match request.method {
                Method::Password => a.password = phc,
                Method::Pin => a.pin = Some(phc),
            },
            None => {
                if next.len() >= ultar_bus::messages::auth::MAX_USERS {
                    return SetOutcome::Rejected;
                }
                // The first account of a device is its administrator.
                let admin = next.is_empty();
                next.push(Account {
                    name: request.user.clone(),
                    display: request.display_name.clone(),
                    admin,
                    password: phc,
                    pin: None,
                });
            }
        }
        match store::save(&self.config.dir, &next) {
            Ok(()) => {
                self.accounts = Ok(next);
                SetOutcome::Done
            }
            Err(_) => SetOutcome::Unavailable,
        }
    }
}
