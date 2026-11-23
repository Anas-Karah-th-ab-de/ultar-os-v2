//! Stateless session challenges and the random parts of account challenges.
//!
//! A session challenge needs no state until it is used. Its id is
//! `0x53 ‖ account (be64) ‖ issue time ms (be64) ‖ public key (32) ‖ nonce (16) ‖ tag (16)`, where
//! `tag` is the first 16 bytes of HMAC-SHA256 under a per-process key over everything before
//! it; the challenge bytes are HMAC-SHA256 under a second per-process key over the same bytes.
//! Both keys come from the operating system's random generator when the process starts, so ids
//! of an earlier process are unknown (no replay across a restart). Standard building blocks only
//! (`hmac`, `sha2`, `rand`).

use crate::challenge::{Bound, Issued, Pending, CHALLENGE_ID_BYTES};
use crate::identity::{CHALLENGE_BYTES, PUBLIC_KEY_BYTES};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::collections::HashMap;

type HmacSha256 = Hmac<Sha256>;

/// Random bytes in a session id.
pub const NONCE_BYTES: usize = 16;
/// Bytes of the authentication tag of a session id.
pub const TAG_BYTES: usize = 16;
const VERSION: u8 = 0x53;
const BODY_BYTES: usize = 1 + 8 + 8 + PUBLIC_KEY_BYTES + NONCE_BYTES;
/// Length of a session id in bytes.
pub const SESSION_ID_BYTES: usize = BODY_BYTES + TAG_BYTES;

/// A session id whose tag verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opened {
    /// What it is bound to.
    pub bound: Bound,
    /// Its nonce (what is remembered once it is used).
    pub nonce: [u8; NONCE_BYTES],
    /// When it was issued.
    pub issued_ms: u64,
}

/// The per-process keys.
pub struct SessionCodec {
    tag_key: [u8; 32],
    challenge_key: [u8; 32],
}

fn mac(key: &[u8; 32], body: &[u8]) -> HmacSha256 {
    // HMAC accepts keys of any length; 32 bytes never fail.
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key)
        .unwrap_or_else(|_| <HmacSha256 as Mac>::new(&Default::default()));
    mac.update(body);
    mac
}

impl SessionCodec {
    /// Fresh keys from the operating system's random generator.
    pub fn random() -> Self {
        let mut rng = rand::rngs::OsRng;
        let (mut tag_key, mut challenge_key) = ([0u8; 32], [0u8; 32]);
        rng.fill_bytes(&mut tag_key);
        rng.fill_bytes(&mut challenge_key);
        Self {
            tag_key,
            challenge_key,
        }
    }

    fn challenge_of(&self, body: &[u8]) -> [u8; CHALLENGE_BYTES] {
        let mut out = [0u8; CHALLENGE_BYTES];
        out.copy_from_slice(&mac(&self.challenge_key, body).finalize().into_bytes());
        out
    }

    /// A session challenge for `public_key` of `account_id`, issued at `now_ms`.
    pub fn issue(
        &self,
        account_id: i64,
        public_key: [u8; PUBLIC_KEY_BYTES],
        now_ms: u64,
    ) -> Issued {
        let mut nonce = [0u8; NONCE_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let mut id = Vec::with_capacity(SESSION_ID_BYTES);
        id.push(VERSION);
        id.extend_from_slice(&account_id.to_be_bytes());
        id.extend_from_slice(&now_ms.to_be_bytes());
        id.extend_from_slice(&public_key);
        id.extend_from_slice(&nonce);
        let challenge = self.challenge_of(&id);
        let tag = mac(&self.tag_key, &id).finalize().into_bytes();
        id.extend_from_slice(&tag[..TAG_BYTES]);
        Issued { id, challenge }
    }

    /// Reads a session id. `None` for anything this process did not issue (wrong length,
    /// version or tag; the tag is compared in constant time).
    pub fn open(&self, id: &[u8]) -> Option<Opened> {
        if id.len() != SESSION_ID_BYTES || id[0] != VERSION {
            return None;
        }
        let (body, tag) = id.split_at(BODY_BYTES);
        mac(&self.tag_key, body).verify_truncated_left(tag).ok()?;
        let field = |from: usize| -> Option<[u8; 8]> { body.get(from..from + 8)?.try_into().ok() };
        let account_id = i64::from_be_bytes(field(1)?);
        let issued_ms = u64::from_be_bytes(field(9)?);
        let public_key: [u8; PUBLIC_KEY_BYTES] =
            body.get(17..17 + PUBLIC_KEY_BYTES)?.try_into().ok()?;
        let nonce: [u8; NONCE_BYTES] = body.get(17 + PUBLIC_KEY_BYTES..)?.try_into().ok()?;
        let bound = Bound {
            account_id,
            public_key,
            challenge: self.challenge_of(body),
        };
        Some(Opened {
            bound,
            nonce,
            issued_ms,
        })
    }
}

/// A fresh random id (not among `pending`) and challenge for an account challenge.
pub(crate) fn random_account_challenge(
    pending: &HashMap<[u8; CHALLENGE_ID_BYTES], Pending>,
) -> ([u8; CHALLENGE_ID_BYTES], [u8; CHALLENGE_BYTES]) {
    let mut rng = rand::rngs::OsRng;
    let mut challenge = [0u8; CHALLENGE_BYTES];
    rng.fill_bytes(&mut challenge);
    let mut id = [0u8; CHALLENGE_ID_BYTES];
    loop {
        rng.fill_bytes(&mut id);
        if !pending.contains_key(&id) {
            return (id, challenge);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_id_round_trips_and_carries_its_binding() {
        let codec = SessionCodec::random();
        let issued = codec.issue(7, [9; 32], 123_456);
        assert_eq!(issued.id.len(), SESSION_ID_BYTES);
        let opened = codec.open(&issued.id).unwrap();
        assert_eq!(opened.bound.account_id, 7);
        assert_eq!(opened.bound.public_key, [9; 32]);
        assert_eq!(opened.bound.challenge, issued.challenge);
        assert_eq!(opened.issued_ms, 123_456);
        let again = codec.issue(7, [9; 32], 123_456);
        assert_ne!(again.id, issued.id);
        assert_ne!(
            again.challenge, issued.challenge,
            "a fresh nonce, a fresh challenge"
        );
    }

    #[test]
    fn any_change_to_an_id_and_ids_of_another_process_are_refused() {
        let codec = SessionCodec::random();
        let issued = codec.issue(-3, [1; 32], 5);
        for i in 0..issued.id.len() {
            let mut bent = issued.id.clone();
            bent[i] ^= 0x01;
            assert!(codec.open(&bent).is_none(), "byte {i}");
        }
        assert!(codec.open(&issued.id[..SESSION_ID_BYTES - 1]).is_none());
        assert!(codec.open(&[]).is_none());
        assert!(SessionCodec::random().open(&issued.id).is_none(), "restart");
    }

    #[test]
    fn account_challenges_are_random_and_avoid_pending_ids() {
        let pending = HashMap::new();
        let (a, ca) = random_account_challenge(&pending);
        let (b, cb) = random_account_challenge(&pending);
        assert_ne!(a, b);
        assert_ne!(ca, cb);
        assert_ne!(ca, [0; 32]);
    }
}
