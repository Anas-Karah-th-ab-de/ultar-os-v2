//! The hash chain that makes the log tamper-evident.
//!
//! Every stored record carries a 32-byte link:
//!
//! ```text
//! link(n) = H(link(n-1) || record bytes of n)
//! ```
//!
//! `H` is SHA-256, or HMAC-SHA-256 when the service has a chain key. A
//! changed, removed, inserted or reordered record breaks every link after
//! it. What each mode proves:
//!
//! * **Without key:** damage and naive edits are detected. Someone who can
//!   rewrite the files *and* recompute all later links is not – nothing in
//!   the files is secret.
//! * **With key** (a secret file only the log service can read): only the
//!   holder of the key can produce links, so a rewritten history is
//!   detected by `ulog verify --key-file`. The head file is authenticated
//!   the same way, which also exposes a cut-off tail.

use sha2::{Digest, Sha256};

/// Length of a link in bytes.
pub const LINK_LEN: usize = 32;
/// A link.
pub type Link = [u8; LINK_LEN];
/// The link before the first record ever written.
pub const GENESIS: Link = [0u8; LINK_LEN];

/// SHA-256 block size.
const BLOCK: usize = 64;

/// Computes links, with or without a key.
#[derive(Clone)]
pub struct Chain {
    /// HMAC pads derived from the key: (inner, outer).
    pads: Option<([u8; BLOCK], [u8; BLOCK])>,
}

impl std::fmt::Debug for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chain")
            .field("keyed", &self.is_keyed())
            .finish()
    }
}

impl Chain {
    /// A chain of plain SHA-256 links.
    pub fn unkeyed() -> Self {
        Self { pads: None }
    }

    /// A chain of HMAC-SHA-256 links under `key`.
    pub fn keyed(key: &[u8]) -> Self {
        let mut block = [0u8; BLOCK];
        if key.len() > BLOCK {
            block[..LINK_LEN].copy_from_slice(&Sha256::digest(key));
        } else {
            block[..key.len()].copy_from_slice(key);
        }
        let mut inner = [0x36u8; BLOCK];
        let mut outer = [0x5cu8; BLOCK];
        for index in 0..BLOCK {
            inner[index] ^= block[index];
            outer[index] ^= block[index];
        }
        Self {
            pads: Some((inner, outer)),
        }
    }

    /// Whether links are authenticated with a key.
    pub fn is_keyed(&self) -> bool {
        self.pads.is_some()
    }

    /// The link of `data` after `previous`.
    pub fn link(&self, previous: &Link, data: &[u8]) -> Link {
        match &self.pads {
            None => {
                let mut hash = Sha256::new();
                hash.update(previous);
                hash.update(data);
                hash.finalize().into()
            }
            Some((inner, outer)) => {
                let mut hash = Sha256::new();
                hash.update(inner);
                hash.update(previous);
                hash.update(data);
                let digest = hash.finalize();
                let mut hash = Sha256::new();
                hash.update(outer);
                hash.update(digest);
                hash.finalize().into()
            }
        }
    }
}

/// Compares two links without stopping at the first difference.
pub fn links_equal(left: &Link, right: &Link) -> bool {
    ultar_service::secret::constant_time_eq(left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn unkeyed_links_are_sha256_of_previous_and_data() {
        // SHA-256 of 32 zero bytes followed by "abc" must match a plain
        // SHA-256 over the same 35 bytes.
        let mut input = vec![0u8; 32];
        input.extend_from_slice(b"abc");
        let expected: Link = Sha256::digest(&input).into();
        assert_eq!(Chain::unkeyed().link(&GENESIS, b"abc"), expected);
        // Known answer for SHA-256("abc") guards the hash itself.
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn keyed_links_are_hmac_sha256_rfc4231_case_2() {
        // RFC 4231 test case 2: key "Jefe", data "what do ya want for
        // nothing?". The link input is previous || data, so the data is
        // split to put its first 32 bytes... it is 28 bytes long; use an
        // empty "previous" by computing over the concatenation directly.
        let chain = Chain::keyed(b"Jefe");
        let (inner, outer) = chain.pads.unwrap();
        let mut hash = Sha256::new();
        hash.update(inner);
        hash.update(b"what do ya want for nothing?");
        let digest = hash.finalize();
        let mut hash = Sha256::new();
        hash.update(outer);
        hash.update(digest);
        assert_eq!(
            hex(&hash.finalize()),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn links_depend_on_previous_data_and_key() {
        let plain = Chain::unkeyed();
        let keyed = Chain::keyed(b"k1");
        let other = Chain::keyed(b"k2");
        let long = Chain::keyed(&[7u8; 100]);
        let base = plain.link(&GENESIS, b"record");
        assert_ne!(base, plain.link(&GENESIS, b"recorD"));
        assert_ne!(base, plain.link(&base, b"record"));
        assert_ne!(base, keyed.link(&GENESIS, b"record"));
        assert_ne!(keyed.link(&GENESIS, b"r"), other.link(&GENESIS, b"r"));
        assert_ne!(keyed.link(&GENESIS, b"r"), long.link(&GENESIS, b"r"));
        assert_eq!(
            keyed.link(&base, b"r"),
            Chain::keyed(b"k1").link(&base, b"r")
        );
        assert!(keyed.is_keyed() && !plain.is_keyed());
        assert!(links_equal(&base, &base) && !links_equal(&base, &GENESIS));
        assert_eq!(format!("{keyed:?}"), "Chain { keyed: true }");
    }
}
