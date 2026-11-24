//! Which devices this device trusts, and how two devices come to trust each other.
//!
//! Pairing: one device shows six digits, the person types them on the other. Both run a
//! password-authenticated key exchange (SPAKE2) over the digits, bound to the two static
//! keys of the encrypted channel. A listener learns nothing about the digits, a device in
//! the middle gets one guess per attempt, and the device that shows the code allows
//! [`MAX_TRIES`] attempts within [`CODE_SECONDS`].

use hmac::{Hmac, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::Right;

/// How long a shown code is valid.
pub const CODE_SECONDS: u32 = 300;
/// Attempts a shown code allows.
pub const MAX_TRIES: u8 = 5;
/// What a freshly paired device may do: offer activities and files (always asked here)
/// and share the clipboard. Seeing, controlling, sound and computing are granted by hand.
pub const DEFAULT_RIGHTS: [Right; 3] = [Right::Handoff, Right::Files, Right::Clipboard];

/// A device this device trusts.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Paired {
    /// Its static public key.
    pub key: Vec<u8>,
    /// Its name when it was last seen (data).
    pub name: String,
    /// Its kind as a word.
    pub kind: String,
    /// What it may do here.
    pub rights: Vec<Right>,
}

/// The paired devices, kept in one file.
#[derive(Debug, Default)]
pub struct Trust {
    peers: BTreeMap<String, Paired>,
    file: Option<PathBuf>,
}

impl Trust {
    /// The devices in `file` (none when it does not exist or cannot be read).
    pub fn open(file: &Path) -> Self {
        let peers = ultar_svckit::store::load(file).unwrap_or_default();
        Self {
            peers,
            file: Some(file.to_owned()),
        }
    }

    /// Trust that is not kept (tests).
    pub fn in_memory() -> Self {
        Self::default()
    }

    fn save(&self) -> bool {
        match &self.file {
            Some(file) => ultar_svckit::store::save(file, &self.peers).is_ok(),
            None => true,
        }
    }

    /// The paired device `id`.
    pub fn get(&self, id: &str) -> Option<&Paired> {
        self.peers.get(id)
    }

    /// All paired devices by id.
    pub fn all(&self) -> impl Iterator<Item = (&String, &Paired)> {
        self.peers.iter()
    }

    /// `true` when `id` is paired and presents exactly the key it was paired with.
    pub fn knows(&self, id: &str, key: &[u8]) -> bool {
        self.peers.get(id).is_some_and(|p| p.key == key)
    }

    /// `true` when the paired device `id` has `right`.
    pub fn allows(&self, id: &str, right: Right) -> bool {
        self.peers
            .get(id)
            .is_some_and(|p| p.rights.contains(&right))
    }

    /// Pairs `id` with the default rights.
    pub fn add(&mut self, id: &str, key: &[u8], name: &str, kind: &str) -> bool {
        self.peers.insert(
            id.to_owned(),
            Paired {
                key: key.to_owned(),
                name: name.to_owned(),
                kind: kind.to_owned(),
                rights: DEFAULT_RIGHTS.to_vec(),
            },
        );
        self.save()
    }

    /// Forgets `id`; `false` when it was not paired.
    pub fn remove(&mut self, id: &str) -> bool {
        self.peers.remove(id).is_some() && self.save()
    }

    /// Sets what `id` may do; `false` when it is not paired.
    pub fn set_rights(&mut self, id: &str, rights: &[Right]) -> bool {
        let Some(peer) = self.peers.get_mut(id) else {
            return false;
        };
        let mut rights = rights.to_vec();
        rights.sort_unstable();
        rights.dedup();
        peer.rights = rights;
        self.save()
    }

    /// Remembers the name and kind a paired device presents now.
    pub fn seen(&mut self, id: &str, name: &str, kind: &str) {
        if let Some(peer) = self.peers.get_mut(id) {
            if peer.name != name || peer.kind != kind {
                peer.name = name.to_owned();
                peer.kind = kind.to_owned();
                let _ = self.save();
            }
        }
    }
}

/// A code this device shows.
#[derive(Debug)]
pub struct Shown {
    code: String,
    until: Instant,
    tries: u8,
}

impl Shown {
    /// A fresh six-digit code, valid from `now`.
    pub fn new(now: Instant) -> Self {
        let number: u32 = rand::thread_rng().gen_range(0..1_000_000);
        Self {
            code: format!("{number:06}"),
            until: now + Duration::from_secs(u64::from(CODE_SECONDS)),
            tries: MAX_TRIES,
        }
    }

    /// The digits.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// `true` while the code can still be used.
    pub fn valid(&self, now: Instant) -> bool {
        self.tries > 0 && now < self.until
    }

    /// Uses up one attempt; the digits when the code was still valid.
    pub fn attempt(&mut self, now: Instant) -> Option<&str> {
        if !self.valid(now) {
            return None;
        }
        self.tries -= 1;
        Some(&self.code)
    }
}

fn exchange(
    code: &str,
    joiner_key: &[u8],
    shower_key: &[u8],
    joiner: bool,
) -> (Spake2<Ed25519Group>, Vec<u8>) {
    let password = Password::new(code.as_bytes());
    let (a, b) = (Identity::new(joiner_key), Identity::new(shower_key));
    if joiner {
        Spake2::<Ed25519Group>::start_a(&password, &a, &b)
    } else {
        Spake2::<Ed25519Group>::start_b(&password, &a, &b)
    }
}

fn proof(key: &[u8], side: &[u8]) -> Option<Vec<u8>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).ok()?;
    mac.update(b"ultar-mesh-pair-v1");
    mac.update(side);
    Some(mac.finalize().into_bytes().to_vec())
}

fn proven(key: &[u8], side: &[u8], given: &[u8]) -> bool {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(key) else {
        return false;
    };
    mac.update(b"ultar-mesh-pair-v1");
    mac.update(side);
    mac.verify_slice(given).is_ok()
}

/// The device that typed the code, between step 1 and step 2.
pub struct Joiner(Spake2<Ed25519Group>);

impl Joiner {
    /// Step 1: the exchange and its first message.
    pub fn start(code: &str, my_key: &[u8], their_key: &[u8]) -> (Self, Vec<u8>) {
        let (state, msg) = exchange(code, my_key, their_key, true);
        (Self(state), msg)
    }

    /// Step 2 arrived: the proof to send back when the other device knew the code.
    pub fn finish(self, msg: &[u8], their_proof: &[u8]) -> Option<Vec<u8>> {
        let key = self.0.finish(msg).ok()?;
        if !proven(&key, b"shower", their_proof) {
            return None;
        }
        proof(&key, b"joiner")
    }
}

/// The device that shows the code, between step 2 and step 3.
pub struct Shower(Vec<u8>);

impl Shower {
    /// Step 1 arrived: the answer `(message, proof)` and what to expect in step 3.
    pub fn answer(
        code: &str,
        my_key: &[u8],
        their_key: &[u8],
        msg: &[u8],
    ) -> Option<(Self, Vec<u8>, Vec<u8>)> {
        let (state, reply) = exchange(code, their_key, my_key, false);
        let key = state.finish(msg).ok()?;
        let mine = proof(&key, b"shower")?;
        Some((Self(key), reply, mine))
    }

    /// Step 3 arrived: `true` when the other device typed the right code.
    pub fn confirmed(&self, their_proof: &[u8]) -> bool {
        proven(&self.0, b"joiner", their_proof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(shown: &str, typed: &str, joiner_sees: &[u8]) -> bool {
        let (jk, sk) = ([1u8; 32], [2u8; 32]);
        let (joiner, m1) = Joiner::start(typed, &jk, joiner_sees);
        let Some((shower, m2, p2)) = Shower::answer(shown, &sk, &jk, &m1) else {
            return false;
        };
        match joiner.finish(&m2, &p2) {
            Some(p3) => shower.confirmed(&p3),
            None => false,
        }
    }

    #[test]
    fn the_right_code_pairs_and_a_wrong_one_does_not() {
        assert!(pair("123456", "123456", &[2u8; 32]));
        assert!(!pair("123456", "123457", &[2u8; 32]));
    }

    #[test]
    fn a_device_in_the_middle_fails_even_with_the_right_code() {
        // The joiner's channel ends at another key than the one that shows the code.
        assert!(!pair("123456", "123456", &[9u8; 32]));
    }

    #[test]
    fn a_code_allows_five_attempts_and_expires() {
        let now = Instant::now();
        let mut shown = Shown::new(now);
        assert_eq!(shown.code().len(), 6);
        for _ in 0..MAX_TRIES {
            assert!(shown.attempt(now).is_some());
        }
        assert!(shown.attempt(now).is_none());
        let mut late = Shown::new(now);
        let after = now + Duration::from_secs(u64::from(CODE_SECONDS) + 1);
        assert!(late.attempt(after).is_none());
    }

    #[test]
    fn trust_is_kept_and_bound_to_the_key() {
        let dir = std::env::temp_dir().join(format!("mesh-trust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("peers");
        let mut trust = Trust::open(&file);
        assert!(trust.add("abc", &[7; 32], "Phone", "phone"));
        assert!(trust.set_rights("abc", &[Right::View, Right::View, Right::Handoff]));
        let again = Trust::open(&file);
        assert!(again.knows("abc", &[7; 32]));
        assert!(!again.knows("abc", &[8; 32]));
        assert!(again.allows("abc", Right::View) && !again.allows("abc", Right::Control));
        assert_eq!(again.get("abc").unwrap().rights.len(), 2);
        let mut again = again;
        assert!(again.remove("abc") && !again.remove("abc"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
