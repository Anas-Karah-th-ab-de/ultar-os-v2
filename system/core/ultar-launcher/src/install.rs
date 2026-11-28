//! Installing a package: its signature, its identity and its place.
//!
//! An app is its id **and** the key that signed it (review L-1):
//!
//! * a store app's id is `<publisher>.<app>`, the publisher being the name
//!   of the store key's file (`acme.ed25519` → `acme.*`) and, if the
//!   manifest names one, its `publisher`; a system app's id has no
//!   publisher part – so no store package can name a system app;
//! * an id the launcher knows (installed, or removed with its user id and
//!   possibly its data kept) is only ever given to a package of the same
//!   signer, and never in a lower version than the one installed last.

use crate::appid::AppId;
use crate::config::Config;
use crate::datadir;
use crate::error::{LaunchError, PackageError};
use crate::launcher::{lock, states, Launcher};
use crate::manifest::{normalised_absolute, Manifest};
use crate::perm::{Permission, PermissionSet};
use crate::profile::ProfileName;
use crate::store::Entry;
use crate::version::order as version_order;
use std::path::Path;
use ultar_bus::messages::launcher::InstallReply;
use ultar_pkg::SignedManifest;

/// Largest package the launcher reads (its signature covers all of it).
pub const MAX_PACKAGE: u64 = 256 << 20;

/// Verifies `package` against the system keys, then the store keys.
fn verify(config: &Config, package: &Path) -> Result<(SignedManifest, ProfileName), PackageError> {
    let meta = std::fs::symlink_metadata(package).map_err(|_| PackageError::Read)?;
    if !meta.file_type().is_file() || meta.len() > MAX_PACKAGE {
        return Err(PackageError::Read);
    }
    let paths = &config.paths;
    if let Ok(signed) = ultar_pkg::verify_signed(package, &paths.system_keys) {
        return Ok((signed, ProfileName::System));
    }
    ultar_pkg::verify_signed(package, &paths.store_keys)
        .map(|signed| (signed, ProfileName::Standard))
        .map_err(|_| PackageError::Signature)
}

/// The signer every app from another system is recorded with (`launcher.install_foreign`):
/// the SHA-256 of a fixed text, so no key of a package can ever be it.
pub fn foreign_signer() -> String {
    crate::store::digest(b"ultar.launcher.install_foreign.v1")
}

/// Whether the id fits who signed the package.
pub fn id_fits_signer(
    id: &AppId,
    publisher: Option<&str>,
    key_name: &str,
    profile: ProfileName,
) -> bool {
    match profile {
        ProfileName::System => !id.as_str().contains('.'),
        ProfileName::Standard => {
            // no store key may be named like the apps of other systems
            let key_ok = AppId::new(key_name)
                .is_ok_and(|k| !k.as_str().contains('.') && k.as_str() != crate::foreign::PREFIX);
            let prefix_ok = id
                .as_str()
                .split_once('.')
                .is_some_and(|(p, _)| p == key_name);
            key_ok && prefix_ok && publisher.is_none_or(|p| p == key_name)
        }
        ProfileName::Foreign => crate::foreign::is_foreign_id(id) && key_name.is_empty(),
    }
}

/// Whether `previous` (a stored signer) names the key of `fingerprint`: the
/// same full identity, or – an entry of state format 2 – its 16-digit short id.
fn same_signer(previous: &str, fingerprint: &str) -> bool {
    fingerprint.len() == 64
        && (previous == fingerprint || (previous.len() == 16 && fingerprint.starts_with(previous)))
}

/// Whether a package signed by the key `fingerprint` (full SHA-256) in
/// `version` may take over `previous`.
pub fn may_replace(previous: &Entry, fingerprint: &str, version: &str) -> Result<(), PackageError> {
    if !same_signer(&previous.signer, fingerprint) {
        return Err(PackageError::Signer);
    }
    match version_order(version, &previous.version) {
        Some(std::cmp::Ordering::Less) => Err(PackageError::Downgrade),
        None if version != previous.version => Err(PackageError::Downgrade),
        _ => Ok(()),
    }
}

impl Launcher {
    /// Verifies and registers the package at `package`.
    pub fn install(&self, package: &str) -> Result<InstallReply, LaunchError> {
        if !normalised_absolute(package) {
            return Err(LaunchError::Invalid);
        }
        let config = lock(&self.core).config.clone();
        let (signed, profile_name) =
            verify(&config, Path::new(package)).map_err(LaunchError::Package)?;
        let manifest = Manifest::parse(&signed.bytes)
            .map_err(|e| LaunchError::Package(PackageError::Manifest(e)))?;
        if !id_fits_signer(
            &manifest.id,
            manifest.publisher.as_deref(),
            &signed.key_name,
            profile_name,
        ) {
            return Err(LaunchError::Package(PackageError::Publisher));
        }
        let profile = profile_name.profile();
        profile
            .admit(&manifest)
            .map_err(|e| LaunchError::Package(PackageError::Profile(e)))?;
        self.register(&manifest, &signed, profile_name)
    }

    /// Registers an app from another system (`launcher.install_foreign`, only callers
    /// that hold `launcher.foreign`): its manifest, made by the service for such apps after
    /// the person confirmed, gets the profile `app.foreign`.
    pub fn install_foreign(&self, manifest: &str) -> Result<InstallReply, LaunchError> {
        let bytes = manifest.as_bytes();
        let parsed = crate::foreign::parse(bytes)
            .map_err(|e| LaunchError::Package(PackageError::Manifest(e)))?;
        let profile = ProfileName::Foreign;
        profile
            .profile()
            .admit(&parsed)
            .map_err(|e| LaunchError::Package(PackageError::Profile(e)))?;
        let fingerprint = foreign_signer();
        let signed = SignedManifest {
            bytes: bytes.to_vec(),
            key_id: fingerprint[..16].to_owned(),
            key_name: String::new(),
            key_fingerprint: fingerprint,
        };
        self.register(&parsed, &signed, profile)
    }

    fn register(
        &self,
        manifest: &Manifest,
        signed: &SignedManifest,
        profile_name: ProfileName,
    ) -> Result<InstallReply, LaunchError> {
        let mut core = lock(&self.core);
        if core.stopping {
            return Err(LaunchError::ShuttingDown);
        }
        let id = manifest.id.clone();
        let previous = core.store.state.get(&id).cloned();
        if let Some(previous) = &previous {
            // an app of one profile never takes over the id of another profile's app
            if previous.profile != profile_name {
                return Err(LaunchError::Package(PackageError::Signer));
            }
            // apps from other systems follow their package manager's versions
            let version = if profile_name == ProfileName::Foreign {
                &previous.version
            } else {
                &manifest.version
            };
            may_replace(previous, &signed.key_fingerprint, version)
                .map_err(LaunchError::Package)?;
        }
        let uid = core.store.state.uid_for(&id)?;
        let owner = core.owner(uid)?;
        datadir::ensure_app_dir(&core.config.paths.data_root, id.as_str(), owner)
            .map_err(|_| LaunchError::StartFailed)?;
        let granted = granted_after(manifest, profile_name, previous);
        let digest = core.store.save_manifest(&id, &signed.bytes)?;
        let entry = Entry {
            uid,
            profile: profile_name,
            installed: true,
            version: manifest.version.clone(),
            digest,
            granted,
            signer: signed.key_fingerprint.clone(),
        };
        core.store.state.put(id.clone(), entry);
        core.store.save()?;
        core.write_rights_or_withdraw()?;
        self.note("launcher.installed", &id);
        Ok(InstallReply {
            app: id.to_string(),
            uid,
            profile: profile_name.word().to_owned(),
            declared: states(manifest.declared, granted),
        })
    }
}

/// What the app holds once installed: an update keeps what the user granted, as far as it
/// is still declared, and the profile adds what it grants at once.
fn granted_after(
    manifest: &Manifest,
    profile_name: ProfileName,
    previous: Option<Entry>,
) -> PermissionSet {
    let kept = previous
        .filter(|e| e.installed)
        .map_or(PermissionSet::EMPTY, |e| e.granted);
    let initial = profile_name.profile().initial_grants(manifest);
    PermissionSet::of(
        &manifest
            .declared
            .iter()
            .filter(|p| kept.has(*p) || initial.has(*p))
            .collect::<Vec<Permission>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::digest;

    fn id(text: &str) -> AppId {
        AppId::new(text).unwrap()
    }

    #[test]
    fn ids_fit_their_signer() {
        let (system, store) = (ProfileName::System, ProfileName::Standard);
        assert!(id_fits_signer(&id("camera"), None, "km", system));
        assert!(!id_fits_signer(&id("acme.camera"), None, "km", system));
        assert!(id_fits_signer(&id("acme.game"), None, "acme", store));
        assert!(id_fits_signer(
            &id("acme.game"),
            Some("acme"),
            "acme",
            store
        ));
        assert!(!id_fits_signer(
            &id("acme.game"),
            Some("other"),
            "acme",
            store
        ));
        assert!(
            !id_fits_signer(&id("camera"), None, "acme", store),
            "a store app cannot take a system id"
        );
        assert!(!id_fits_signer(&id("other.game"), None, "acme", store));
        assert!(!id_fits_signer(&id("acme.game"), None, "Acme", store));
        assert!(!id_fits_signer(&id("acme.game"), None, "", store));
    }

    #[test]
    fn only_the_same_signer_replaces_and_never_downwards() {
        let full = "0123456789abcdef".repeat(4);
        let mut previous = Entry {
            uid: 20000,
            profile: ProfileName::System,
            installed: false,
            version: "1.4.0".into(),
            digest: digest(b"m"),
            granted: PermissionSet::EMPTY,
            signer: full.clone(),
        };
        assert_eq!(may_replace(&previous, &full, "1.4.0"), Ok(()));
        assert_eq!(may_replace(&previous, &full, "1.10"), Ok(()));
        // R2-2: a key whose short id is the same but whose full identity differs is another signer
        let twin = format!("0123456789abcdef{}", "f".repeat(48));
        assert_eq!(
            may_replace(&previous, &twin, "9.0"),
            Err(PackageError::Signer)
        );
        assert_eq!(
            may_replace(&previous, &"fedcba9876543210".repeat(4), "9.0"),
            Err(PackageError::Signer)
        );
        assert_eq!(
            may_replace(&previous, &full, "1.3.9"),
            Err(PackageError::Downgrade)
        );
        assert_eq!(
            may_replace(&previous, &full, "1.4.x"),
            Err(PackageError::Downgrade)
        );
        // an entry of state format 2 holds the short id: its own key still replaces it
        previous.signer = "0123456789abcdef".into();
        assert_eq!(may_replace(&previous, &full, "1.5"), Ok(()));
        assert_eq!(
            may_replace(&previous, "0123456789abcdef", "1.5"),
            Err(PackageError::Signer),
            "a short id is never the new signer"
        );
    }
}
