//! Who is on the other end of a connection.
//!
//! Every listener of the broker owns one [`PeerIdentity`] implementation that
//! maps an accepted connection to an [`Identity`]. The [`crate::Policy`] then
//! decides what that identity may do.

use crate::error::BusError;
use crate::topic::check_name;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::os::unix::net::UnixStream;

/// Name of an authenticated peer, e.g. `system.settingsd` or `app.phone`.
///
/// Identities use the same hierarchical syntax as topics so that policy rules
/// can address whole groups (`app.*`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Identity(String);

impl Identity {
    /// Creates an identity from a valid hierarchical name.
    pub fn new(name: &str) -> Result<Self, BusError> {
        check_name(name).map(|name| Self(name.to_owned()))
    }

    /// The textual name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Identity {
    type Error = BusError;

    fn try_from(name: String) -> Result<Self, BusError> {
        Self::new(&name)
    }
}

impl From<Identity> for String {
    fn from(identity: Identity) -> String {
        identity.0
    }
}

/// What the broker knows about a peer when it has to identify it.
#[derive(Debug)]
pub struct PeerInfo<'a> {
    /// The accepted connection (for credential lookups).
    pub stream: &'a UnixStream,
    /// Diagnostic program name sent by the client; **not** trustworthy.
    pub client_name: &'a str,
    /// Secret sent by the client, if any.
    pub token: Option<&'a str>,
}

/// Maps an accepted connection to an [`Identity`].
pub trait PeerIdentity: Send + Sync {
    /// Returns the identity of the peer or [`BusError::AuthFailed`].
    fn identify(&self, peer: &PeerInfo<'_>) -> Result<Identity, BusError>;
}

/// Every connection on the listener gets the same identity.
///
/// The operating system enforces who may connect: the socket file (or the
/// directory it lives in, or the mount that exposes it inside an app cell)
/// is only reachable by the intended user.
#[derive(Clone, Debug)]
pub struct FixedIdentity(Identity);

impl FixedIdentity {
    /// All peers of the listener are `identity`.
    pub fn new(identity: Identity) -> Self {
        Self(identity)
    }
}

impl PeerIdentity for FixedIdentity {
    fn identify(&self, _peer: &PeerInfo<'_>) -> Result<Identity, BusError> {
        Ok(self.0.clone())
    }
}

/// Identifies peers by a secret they present in the handshake.
///
/// The service manager hands each program its token through a file only that
/// program's user can read.
#[derive(Clone, Debug, Default)]
pub struct TokenIdentity {
    tokens: Vec<(String, Identity)>,
}

impl TokenIdentity {
    /// An authenticator that knows no token yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a token; empty tokens are ignored because they would match
    /// clients that present nothing.
    pub fn with_token(mut self, token: &str, identity: Identity) -> Self {
        if !token.is_empty() {
            self.tokens.push((token.to_owned(), identity));
        }
        self
    }
}

/// Compares two secrets without stopping at the first differing byte.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for (index, byte) in left.iter().enumerate() {
        let other = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(byte ^ other);
    }
    difference == 0
}

impl PeerIdentity for TokenIdentity {
    fn identify(&self, peer: &PeerInfo<'_>) -> Result<Identity, BusError> {
        let presented = peer.token.ok_or(BusError::AuthFailed)?;
        let mut found = None;
        for (token, identity) in &self.tokens {
            if constant_time_eq(token.as_bytes(), presented.as_bytes()) {
                found = Some(identity.clone());
            }
        }
        found.ok_or(BusError::AuthFailed)
    }
}

/// Kernel-reported credentials of a socket peer (`SO_PEERCRED`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PeerCredentials {
    /// Effective user id of the peer process.
    pub uid: u32,
    /// Effective group id of the peer process.
    pub gid: u32,
    /// Process id of the peer, when the kernel reports it.
    pub pid: Option<u32>,
}

/// Reads the kernel credentials of a connection.
///
/// `UnixStream::peer_cred` is not stable on the pinned toolchain and this
/// crate forbids `unsafe`, so the real `SO_PEERCRED` reader lives in the HAL
/// and is plugged in through this trait.
pub trait CredentialSource: Send + Sync {
    /// Returns the credentials of the peer of `stream`.
    fn credentials(&self, stream: &UnixStream) -> Result<PeerCredentials, BusError>;
}

/// A [`CredentialSource`] that reports the same credentials for every
/// connection. Meant for tests and single-user development setups.
#[derive(Clone, Copy, Debug)]
pub struct StaticCredentials(pub PeerCredentials);

impl CredentialSource for StaticCredentials {
    fn credentials(&self, _stream: &UnixStream) -> Result<PeerCredentials, BusError> {
        Ok(self.0)
    }
}

/// Identifies peers by the user id the kernel reports for the connection.
pub struct CredentialIdentity<S> {
    source: S,
    by_uid: HashMap<u32, Identity>,
}

impl<S: CredentialSource> CredentialIdentity<S> {
    /// Uses `source` to read credentials; no user id is known yet.
    pub fn new(source: S) -> Self {
        Self {
            source,
            by_uid: HashMap::new(),
        }
    }

    /// Maps the user id `uid` to `identity`.
    pub fn with_uid(mut self, uid: u32, identity: Identity) -> Self {
        self.by_uid.insert(uid, identity);
        self
    }
}

impl<S: CredentialSource> PeerIdentity for CredentialIdentity<S> {
    fn identify(&self, peer: &PeerInfo<'_>) -> Result<Identity, BusError> {
        let credentials = self.source.credentials(peer.stream)?;
        self.by_uid
            .get(&credentials.uid)
            .cloned()
            .ok_or(BusError::AuthFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> Identity {
        Identity::new(name).unwrap()
    }

    fn with_peer<R>(token: Option<&str>, check: impl FnOnce(&PeerInfo<'_>) -> R) -> R {
        let (stream, _other) = UnixStream::pair().unwrap();
        check(&PeerInfo {
            stream: &stream,
            client_name: "test",
            token,
        })
    }

    #[test]
    fn identity_names_are_validated() {
        assert_eq!(id("system.devd").as_str(), "system.devd");
        assert_eq!(id("app.phone").to_string(), "app.phone");
        assert!(Identity::new("Not Valid").is_err());
        assert!(Identity::new("app.*").is_err());
        assert!(Identity::try_from(String::from("ok")).is_ok());
        assert_eq!(String::from(id("a.b")), "a.b");
    }

    #[test]
    fn fixed_identity_accepts_everyone() {
        let auth = FixedIdentity::new(id("ui"));
        assert_eq!(
            with_peer(None, |peer| auth.identify(peer)).unwrap(),
            id("ui")
        );
    }

    #[test]
    fn token_identity_requires_the_right_secret() {
        let auth = TokenIdentity::new()
            .with_token("alpha-secret", id("system.devd"))
            .with_token("beta-secret", id("app.phone"))
            .with_token("", id("nobody"));
        assert_eq!(
            with_peer(Some("beta-secret"), |peer| auth.identify(peer)).unwrap(),
            id("app.phone")
        );
        for wrong in [None, Some(""), Some("alpha"), Some("alpha-secret2")] {
            let result = with_peer(wrong, |peer| auth.identify(peer));
            assert!(matches!(result, Err(BusError::AuthFailed)), "{wrong:?}");
        }
    }

    #[test]
    fn constant_time_eq_is_correct() {
        assert!(constant_time_eq(b"", b""));
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"ab", b"abc"));
    }

    #[test]
    fn credential_identity_maps_user_ids() {
        let credentials = PeerCredentials {
            uid: 1000,
            gid: 1000,
            pid: Some(4),
        };
        let known = CredentialIdentity::new(StaticCredentials(credentials))
            .with_uid(0, id("system"))
            .with_uid(1000, id("ui"));
        assert_eq!(
            with_peer(None, |peer| known.identify(peer)).unwrap(),
            id("ui")
        );
        let unknown = CredentialIdentity::new(StaticCredentials(credentials)).with_uid(0, id("x"));
        assert!(matches!(
            with_peer(None, |peer| unknown.identify(peer)),
            Err(BusError::AuthFailed)
        ));
    }
}
