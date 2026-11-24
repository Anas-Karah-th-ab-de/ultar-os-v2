//! Catalogue signatures: Ed25519 (`ed25519-dalek`, strict verification) over a
//! domain-separated digest, checked against the device's trusted keys – the same
//! key directory and blob layout as `.uap` packages (`ultar-pkg`), with its own
//! prefix so a package signature is never a catalogue signature:
//!
//! ```text
//! message = "ultar-catalog-v1\n" + hex(sha256(body)) + "\n"
//! file <body>.sig = "<key-id>\n" + base64(64-byte signature)
//! ```
//!
//! Key files are only trusted when nobody but their owner can change them
//! (regular file, not a symlink, no group/other write bit, owned by root or by
//! the directory's owner; the directory itself not a symlink and not
//! group/other-writable). Anything else is skipped.
//!
//! **Revocation:** lines `revoke <hex public key>` in any key file of the directory (by
//! convention `revoked.list`, shipped with the image like the keys) remove that key from the
//! trusted set, even when another file still lists it. Such a line is never itself a key.

use crate::error::SourceError;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Prefix of the signed message; keeps catalogue signatures out of the package domain.
pub const CATALOG_MAGIC: &str = "ultar-catalog-v1";
/// Largest key file read from the trusted-keys directory.
const MAX_KEY_FILE: u64 = 64 * 1024;

/// The canonical message that is signed over a catalogue body.
pub fn catalog_message(body: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(body);
    format!("{CATALOG_MAGIC}\n{}\n", hex::encode(h.finalize())).into_bytes()
}

/// The key id of a public key (first 16 hex of sha256(pubkey), as `.uap`).
pub fn key_id(pubkey: &VerifyingKey) -> String {
    let mut h = Sha256::new();
    h.update(pubkey.as_bytes());
    let mut id = hex::encode(h.finalize());
    id.truncate(16);
    id
}

/// Signature blob text for a signature: `"<key-id>\n<base64 sig>"`.
pub fn signature_blob(pubkey: &VerifyingKey, sig: &Signature) -> String {
    format!("{}\n{}", key_id(pubkey), B64.encode(sig.to_bytes()))
}

/// Signs a catalogue body, returning the detached signature blob. The private
/// key never lives in the running app; this is for the publisher tool and the
/// stick-preparation tests.
pub fn sign_catalog(body: &[u8], signing: &ed25519_dalek::SigningKey) -> String {
    use ed25519_dalek::Signer;
    let sig = signing.sign(&catalog_message(body));
    signature_blob(&signing.verifying_key(), &sig)
}

/// Not writable by group or others.
#[cfg(unix)]
fn owner_only(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.mode() & 0o022 == 0
}

#[cfg(not(unix))]
fn owner_only(_meta: &std::fs::Metadata) -> bool {
    true
}

/// A key file must belong to root or to the owner of its directory.
#[cfg(unix)]
fn same_owner(file: &std::fs::Metadata, dir: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    file.uid() == 0 || file.uid() == dir.uid()
}

#[cfg(not(unix))]
fn same_owner(_file: &std::fs::Metadata, _dir: &std::fs::Metadata) -> bool {
    true
}

/// Reads the hex public keys of a trusted-keys directory (same files as
/// `ultar-pkg`). Writable-by-others directories and files are ignored.
pub fn trusted_keys(dir: &Path) -> Vec<VerifyingKey> {
    let mut keys = Vec::new();
    let mut revoked = Vec::new();
    let dir_meta = match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() && owner_only(&meta) => meta,
        _ => return keys,
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return keys;
    };
    for entry in rd.flatten() {
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !meta.is_file()
            || meta.len() > MAX_KEY_FILE
            || !owner_only(&meta)
            || !same_owner(&meta, &dir_meta)
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        keys.extend(text.lines().filter_map(parse_key));
        revoked.extend(
            text.lines()
                .filter_map(|l| l.trim().strip_prefix("revoke "))
                .filter_map(parse_key),
        );
    }
    keys.retain(|k| !revoked.contains(k));
    keys
}

fn parse_key(line: &str) -> Option<VerifyingKey> {
    let bytes = hex::decode(line.trim()).ok()?;
    let arr = <[u8; 32]>::try_from(bytes.as_slice()).ok()?;
    VerifyingKey::from_bytes(&arr).ok()
}

/// Verifies a detached signature over `body` against the trusted keys and
/// answers the key id of the key that actually verified it (never the id the
/// blob merely claims).
pub fn verify_signature(
    body: &[u8],
    blob: &str,
    trusted_dir: &Path,
) -> Result<Option<String>, SourceError> {
    let sig_b64 = blob.lines().nth(1).unwrap_or("").trim();
    let bytes = B64.decode(sig_b64).map_err(|_| SourceError::BadSignature)?;
    let arr = <[u8; 64]>::try_from(bytes.as_slice()).map_err(|_| SourceError::BadSignature)?;
    let sig = Signature::from_bytes(&arr);
    let msg = catalog_message(body);
    trusted_keys(trusted_dir)
        .iter()
        .find(|k| k.verify_strict(&msg, &sig).is_ok())
        .map(|k| Some(key_id(k)))
        .ok_or(SourceError::UnknownSigner)
}

#[cfg(test)]
#[path = "signature_tests.rs"]
mod tests;
