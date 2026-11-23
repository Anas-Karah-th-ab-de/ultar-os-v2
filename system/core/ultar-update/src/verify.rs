//! Verification of a signed manifest – the gate in front of every slot write.
//!
//! Two stages: [`authenticate`] proves who made the manifest (signature, canonical form);
//! [`admit`] decides whether this device may install it now. The result, a
//! [`VerifiedManifest`], cannot be built in any other way, and the slot writer accepts nothing
//! else.

use crate::error::UpdateError;
use crate::manifest::{Manifest, Role, MAX_MANIFEST};
use crate::sign::{self, PublicKey, MANIFEST_DOMAIN, MANIFEST_MAGIC};
use crate::version::Version;
use sha2::{Digest, Sha256};

/// Default grace period after `expires` (clock differences), in seconds.
pub const DEFAULT_EXPIRY_TOLERANCE: u64 = 24 * 60 * 60;

/// What this device is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    /// Processor architecture, e.g. `x86_64`.
    pub arch: String,
    /// Edition, e.g. `desktop`.
    pub edition: String,
    /// Device model identifier, e.g. `pc-uefi`.
    pub device: String,
}

/// A manifest whose signature verified against the trusted keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authentic {
    manifest: Manifest,
    signed: Vec<u8>,
}

impl Authentic {
    /// The manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The signed envelope bytes.
    pub fn signed(&self) -> &[u8] {
        &self.signed
    }

    /// SHA-256 of the signed envelope: the identity of this release on the device.
    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(&self.signed).into()
    }
}

/// The facts a manifest is judged against.
#[derive(Debug, Clone)]
pub struct Admission<'a> {
    /// This device.
    pub device: &'a DeviceIdentity,
    /// Payload roles a slot of this device consists of.
    pub roles: &'a [Role],
    /// Stored rollback floor.
    pub floor: u64,
    /// Running version.
    pub current: Version,
    /// Time to judge expiry with (clock, bounded below by the time floor).
    pub now: u64,
    /// Grace period after `expires` in seconds.
    pub tolerance: u64,
    /// The channel that was asked for (online); `None` accepts any (offline bundle).
    pub channel: Option<&'a str>,
    /// An administrator explicitly allowed an older version (never below the floor).
    pub allow_downgrade: bool,
}

/// A manifest this device may install now. Only [`admit`] creates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedManifest {
    inner: Authentic,
}

impl VerifiedManifest {
    /// The manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.inner.manifest
    }

    /// The signed envelope bytes.
    pub fn signed(&self) -> &[u8] {
        &self.inner.signed
    }

    /// SHA-256 of the signed envelope.
    pub fn digest(&self) -> [u8; 32] {
        self.inner.digest()
    }
}

/// Stage 1: envelope shape, known key, signature, canonical manifest – in this order.
pub fn authenticate(signed: &[u8], keys: &[PublicKey]) -> Result<Authentic, UpdateError> {
    let envelope = sign::open_exact(&MANIFEST_MAGIC, signed, MAX_MANIFEST)?;
    envelope.verify(MANIFEST_DOMAIN, keys)?;
    let manifest = Manifest::parse(envelope.body)?;
    Ok(Authentic {
        manifest,
        signed: signed.to_vec(),
    })
}

/// Stage 2: expiry, architecture, edition, device, channel, payload roles, rollback index,
/// minimum version, version order – the first failure wins.
pub fn admit(authentic: Authentic, facts: &Admission<'_>) -> Result<VerifiedManifest, UpdateError> {
    let m = &authentic.manifest;
    if facts.now > m.expires.saturating_add(facts.tolerance) {
        return Err(UpdateError::Expired);
    }
    if m.arch != facts.device.arch {
        return Err(UpdateError::WrongArch);
    }
    if m.edition != facts.device.edition {
        return Err(UpdateError::WrongEdition);
    }
    if !m.allows_device(&facts.device.device) {
        return Err(UpdateError::WrongDevice);
    }
    if facts.channel.is_some_and(|c| c != m.channel) {
        return Err(UpdateError::WrongChannel);
    }
    let mut wanted = facts.roles.to_vec();
    wanted.sort_unstable();
    wanted.dedup();
    if !m.payloads.iter().map(|p| p.role).eq(wanted) {
        return Err(UpdateError::PayloadSetMismatch);
    }
    if m.rollback_index < facts.floor {
        return Err(UpdateError::RollbackRefused {
            floor: facts.floor,
            offered: m.rollback_index,
        });
    }
    if facts.current < m.min_version {
        return Err(UpdateError::NeedsIntermediate);
    }
    if m.version == facts.current {
        return Err(UpdateError::UpToDate);
    }
    if m.version < facts.current && !facts.allow_downgrade {
        return Err(UpdateError::DowngradeRefused);
    }
    Ok(VerifiedManifest { inner: authentic })
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
