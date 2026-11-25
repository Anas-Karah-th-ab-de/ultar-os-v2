//! The signed-in people, without the bus.

use serde::{Deserialize, Serialize};
use ultar_bus::messages::session::{SessionState, MAX_SIGNED_IN};

/// Why a change was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionError {
    /// [`MAX_SIGNED_IN`] people are signed in already.
    Full,
    /// Nobody is in front.
    NobodyActive,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => f.write_str("too many people signed in"),
            Self::NobodyActive => f.write_str("nobody is signed in"),
        }
    }
}

impl std::error::Error for SessionError {}

/// Who is signed in, in sign-in order, and who is in front.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Sessions {
    signed_in: Vec<String>,
    active: Option<String>,
}

impl Sessions {
    /// The bus view.
    pub fn state(&self) -> SessionState {
        SessionState {
            user: self.active.clone(),
            signed_in: self.signed_in.clone(),
        }
    }

    /// The person in front.
    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// Everybody signed in.
    pub fn signed_in(&self) -> &[String] {
        &self.signed_in
    }

    /// `user` signed in (or came back to the front).
    pub fn sign_in(&mut self, user: &str) -> Result<(), SessionError> {
        if !self.signed_in.iter().any(|u| u == user) {
            if self.signed_in.len() >= MAX_SIGNED_IN {
                return Err(SessionError::Full);
            }
            self.signed_in.push(user.to_owned());
        }
        self.active = Some(user.to_owned());
        Ok(())
    }

    /// The person in front leaves the front (switching users): signed in, not active.
    pub fn leave_front(&mut self) {
        self.active = None;
    }

    /// The person in front signs out; returns who it was.
    pub fn sign_out(&mut self) -> Result<String, SessionError> {
        let user = self.active.take().ok_or(SessionError::NobodyActive)?;
        self.signed_in.retain(|u| *u != user);
        Ok(user)
    }

    /// Ends one person's session (the parental window ended): signed out, and nobody
    /// in front when it was them. `true` when they were signed in.
    pub fn end(&mut self, user: &str) -> bool {
        let was = self.signed_in.iter().any(|u| u == user);
        self.signed_in.retain(|u| u != user);
        if self.active.as_deref() == Some(user) {
            self.active = None;
        }
        was
    }

    /// Drops people whose account no longer exists (removed by an administrator).
    pub fn keep_only(&mut self, accounts: &[String]) {
        self.signed_in.retain(|u| accounts.contains(u));
        if self.active.as_ref().is_some_and(|a| !accounts.contains(a)) {
            self.active = None;
        }
    }

    /// A state read from disk is checked before it is used.
    pub fn is_valid(&self) -> bool {
        let unique = self
            .signed_in
            .iter()
            .enumerate()
            .all(|(i, u)| !self.signed_in[..i].contains(u));
        unique
            && self.signed_in.len() <= MAX_SIGNED_IN
            && self
                .active
                .as_ref()
                .is_none_or(|a| self.signed_in.contains(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_switch_and_sign_out() {
        let mut s = Sessions::default();
        assert_eq!(s.sign_out(), Err(SessionError::NobodyActive));
        s.sign_in("amal").unwrap();
        s.sign_in("badr").unwrap();
        assert_eq!(s.active(), Some("badr"));
        s.leave_front();
        assert_eq!(s.state().user, None);
        s.sign_in("amal").unwrap();
        assert_eq!(s.signed_in(), ["amal", "badr"]);
        assert_eq!(s.sign_out().unwrap(), "amal");
        assert_eq!(s.state().signed_in, vec!["badr".to_owned()]);
        assert!(s.is_valid());
    }

    #[test]
    fn ending_one_session_leaves_the_others() {
        let mut s = Sessions::default();
        s.sign_in("amal").unwrap();
        s.sign_in("badr").unwrap();
        assert!(!s.end("ghost"));
        assert!(s.end("badr"), "the one in front");
        assert_eq!(s.active(), None);
        assert_eq!(s.signed_in(), ["amal"]);
        assert!(s.end("amal"));
        assert!(s.signed_in().is_empty());
        assert!(!s.end("amal"), "already gone");
    }

    #[test]
    fn limits_removed_accounts_and_damaged_state() {
        let mut s = Sessions::default();
        for i in 0..MAX_SIGNED_IN {
            s.sign_in(&format!("u{i}")).unwrap();
        }
        assert_eq!(s.sign_in("one-more"), Err(SessionError::Full));
        s.sign_in("u0").unwrap();
        s.keep_only(&["u1".to_owned()]);
        assert_eq!((s.signed_in().len(), s.active()), (1, None));
        let bad = Sessions {
            signed_in: vec!["a".into(), "a".into()],
            active: None,
        };
        assert!(!bad.is_valid());
        let bad = Sessions {
            signed_in: vec![],
            active: Some("ghost".into()),
        };
        assert!(!bad.is_valid());
        assert!(!SessionError::Full.to_string().is_empty());
        assert!(!SessionError::NobodyActive.to_string().is_empty());
    }
}
