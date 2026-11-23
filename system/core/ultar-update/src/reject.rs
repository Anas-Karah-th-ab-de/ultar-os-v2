//! Remembering a release this device has given up, so that the automatic path does not
//! install it again. Further `impl` block of [`Engine`].
//!
//! One record: the SHA-256 of the signed manifest of the release that failed its health
//! check, never started, or was left by an administrator's "start previous version". It is
//! cleared when any release is confirmed. An administrator may still install the recorded
//! release by hand; only the device's own initiative is stopped.

use crate::bootctl::{BootControl, Slot};
use crate::engine::Engine;
use crate::error::UpdateError;
use crate::fsutil::{read_bounded, remove_if_present, write_atomic};
use crate::stage::StagedInfo;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

impl<B: BootControl> Engine<B> {
    fn rejected_path(&self) -> PathBuf {
        self.config.state_dir.join("rejected")
    }

    /// Digest of the signed manifest of the release that was given up last, if any.
    pub fn rejected_digest(&self) -> Option<[u8; 32]> {
        let bytes = read_bounded(&self.rejected_path(), 80).ok().flatten()?;
        let text = std::str::from_utf8(&bytes).ok()?;
        let mut digest = [0u8; 32];
        hex::decode_to_slice(text.strip_suffix('\n')?, &mut digest).ok()?;
        Some(digest)
    }

    /// True when `digest` is the release this device has given up.
    pub fn is_rejected(&self, digest: &[u8; 32]) -> bool {
        self.rejected_digest().as_ref() == Some(digest)
    }

    /// Records the release in `slot` as given up and empties the staging area (a staged copy
    /// of it must not be picked up again by the automatic path). A slot without a stored
    /// manifest (factory system) leaves no record.
    pub(crate) fn reject_slot(&self, slot: Slot) -> Result<(), UpdateError> {
        let path = self.slot_manifest_path(slot);
        if let Some(signed) = read_bounded(&path, 32 * 1024).ok().flatten() {
            let digest = hex::encode(Sha256::digest(&signed));
            write_atomic(&self.rejected_path(), format!("{digest}\n").as_bytes())?;
        }
        self.staging.purge()
    }

    /// Forgets the given-up release (a release has been confirmed since).
    pub(crate) fn clear_rejected(&self) -> Result<(), UpdateError> {
        Ok(remove_if_present(&self.rejected_path())?)
    }

    /// The complete bundle in the staging area, if there is one.
    pub fn staged(&self) -> Result<Option<StagedInfo>, UpdateError> {
        self.staging.staged()
    }
}
