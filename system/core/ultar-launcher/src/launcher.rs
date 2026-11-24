//! The launcher: installed apps, their grants and their running instances,
//! behind one lock. Starting and watching cells is in [`crate::instances`],
//! the cell itself in [`crate::cell`].

use crate::appid::AppId;
use crate::config::Config;
use crate::datadir::{self, Owner};
use crate::error::{LaunchError, PackageError};
use crate::instances::Instance;
use crate::manifest::{normalised_absolute, Manifest};
use crate::perm::{Permission, PermissionSet};
use crate::platform::{Feature, Platform};
use crate::profile::ProfileName;
use crate::rightsfile;
use crate::store::{Entry, StateError, Store};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use ultar_bus::messages::launcher::{
    InstallReply, PermissionReply, PermissionState, PermissionsReply, Role,
};
use ultar_init::output::Forwarder;
use ultar_init::spec::Slice;
use ultar_service::log::{Level, Logger, Record};

/// Largest package the launcher reads (its signature covers all of it).
pub const MAX_PACKAGE: u64 = 256 << 20;

/// State behind the lock.
pub(crate) struct Core {
    pub(crate) config: Config,
    pub(crate) platform: Platform,
    pub(crate) store: Store,
    pub(crate) instances: BTreeMap<(AppId, Role), Instance>,
    pub(crate) generation: u64,
    pub(crate) stopping: bool,
}

/// The app launcher.
pub struct Launcher {
    pub(crate) core: Mutex<Core>,
    pub(crate) wake: Condvar,
    pub(crate) helper: PathBuf,
    pub(crate) logger: Logger,
    pub(crate) output: Mutex<Forwarder>,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The app id in a request.
pub(crate) fn app_id(text: &str) -> Result<AppId, LaunchError> {
    AppId::new(text).map_err(|_| LaunchError::Invalid)
}

fn states(declared: PermissionSet, granted: PermissionSet) -> Vec<PermissionState> {
    declared
        .iter()
        .map(|p| PermissionState {
            name: p.name().to_owned(),
            granted: granted.has(p),
        })
        .collect()
}

/// Verifies `package` against the system keys, then the store keys.
fn verify(config: &Config, package: &Path) -> Result<(Vec<u8>, ProfileName), PackageError> {
    let meta = std::fs::symlink_metadata(package).map_err(|_| PackageError::Read)?;
    if !meta.file_type().is_file() || meta.len() > MAX_PACKAGE {
        return Err(PackageError::Read);
    }
    let paths = &config.paths;
    if let Ok(bytes) = ultar_pkg::verify_manifest_bytes(package, &paths.system_keys) {
        return Ok((bytes, ProfileName::System));
    }
    ultar_pkg::verify_manifest_bytes(package, &paths.store_keys)
        .map(|bytes| (bytes, ProfileName::Standard))
        .map_err(|_| PackageError::Signature)
}

impl Core {
    pub(crate) fn installed(&self, id: &AppId) -> Result<&Entry, LaunchError> {
        self.store
            .state
            .get(id)
            .filter(|e| e.installed)
            .ok_or(LaunchError::UnknownApp)
    }

    pub(crate) fn manifest(&self, id: &AppId) -> Result<Manifest, LaunchError> {
        let bytes = self.store.load_manifest(id)?;
        Manifest::parse(&bytes).map_err(|e| LaunchError::Package(PackageError::Manifest(e)))
    }

    /// Owner of the app's files, if ids can be switched (else `None`, only
    /// where the operator accepted that).
    pub(crate) fn owner(&self, uid: u32) -> Result<Option<Owner>, LaunchError> {
        let ids = self
            .platform
            .require(Feature::Ids, &self.config.accept_missing);
        Ok(ids
            .map_err(LaunchError::Platform)?
            .then_some(Owner { uid, gid: uid }))
    }

    /// Writes the compositor's app rights file.
    pub(crate) fn write_rights(&self) -> Result<(), LaunchError> {
        let Some(path) = &self.config.paths.compositor_rights else {
            return Ok(());
        };
        let apps: Vec<(u32, PermissionSet)> = self
            .store
            .state
            .apps()
            .filter(|(_, e)| e.installed)
            .map(|(_, e)| (e.uid, e.granted))
            .collect();
        rightsfile::write(path, &apps).map_err(|e| LaunchError::Storage(e.into()))
    }
}

impl Launcher {
    /// Opens the launcher: checks its directories, reads its state (a
    /// damaged state is an error, never reset), writes the compositor's
    /// app rights. `helper` is this program (the cell stages).
    pub fn open(
        config: Config,
        platform: Platform,
        helper: PathBuf,
        logger: Logger,
    ) -> Result<std::sync::Arc<Self>, LaunchError> {
        let dir_error =
            |_| LaunchError::Storage(StateError::Io(std::io::ErrorKind::PermissionDenied));
        datadir::ensure_private_dir(&config.paths.data_root).map_err(dir_error)?;
        datadir::ensure_private_dir(&config.paths.cell_root).map_err(dir_error)?;
        let store = Store::open(&config.paths.state_dir, config.uids)?;
        let output = Forwarder::new(config.paths.output_socket.clone());
        let core = Core {
            config,
            platform,
            store,
            instances: BTreeMap::new(),
            generation: 0,
            stopping: false,
        };
        core.write_rights()?;
        let record = Record::new(Level::Info, "launcher.platform")
            .flag("ultar", platform.ultar)
            .flag("ids", platform.ids)
            .flag("cell", platform.cell)
            .flag("groups", platform.groups)
            .flag("net", platform.net)
            .flag("file_binds", platform.file_binds);
        logger.emit(record);
        Ok(std::sync::Arc::new(Self {
            core: Mutex::new(core),
            wake: Condvar::new(),
            helper,
            logger,
            output: Mutex::new(output),
        }))
    }

    /// What the kernel offers (as detected at start).
    pub fn platform(&self) -> Platform {
        lock(&self.core).platform
    }

    pub(crate) fn refused(&self, caller: &str) {
        self.logger
            .emit(Record::new(Level::Warn, "launcher.refused").id_or_invalid("caller", caller));
    }

    pub(crate) fn note(&self, key: &'static str, app: &AppId) {
        self.logger
            .emit(Record::new(Level::Info, key).id_or_invalid("app", app.as_str()));
    }

    /// Verifies and registers the package at `package`.
    pub fn install(&self, package: &str) -> Result<InstallReply, LaunchError> {
        if !normalised_absolute(package) {
            return Err(LaunchError::Invalid);
        }
        let config = lock(&self.core).config.clone();
        let (bytes, profile_name) =
            verify(&config, Path::new(package)).map_err(LaunchError::Package)?;
        let manifest =
            Manifest::parse(&bytes).map_err(|e| LaunchError::Package(PackageError::Manifest(e)))?;
        let profile = profile_name.profile();
        profile
            .admit(&manifest)
            .map_err(|e| LaunchError::Package(PackageError::Profile(e)))?;
        let mut core = lock(&self.core);
        if core.stopping {
            return Err(LaunchError::ShuttingDown);
        }
        let id = manifest.id.clone();
        let uid = core.store.state.uid_for(&id)?;
        let owner = core.owner(uid)?;
        datadir::ensure_app_dir(&core.config.paths.data_root, id.as_str(), owner)
            .map_err(|_| LaunchError::StartFailed)?;
        // an update keeps what the user granted, as far as it is still declared
        let kept = core
            .store
            .state
            .get(&id)
            .filter(|e| e.installed)
            .map_or(PermissionSet::EMPTY, |e| e.granted);
        let granted = PermissionSet::of(
            &manifest
                .declared
                .iter()
                .filter(|p| kept.has(*p) || profile.initial_grants(&manifest).has(*p))
                .collect::<Vec<Permission>>(),
        );
        let digest = core.store.save_manifest(&id, &bytes)?;
        let entry = Entry {
            uid,
            profile: profile_name,
            installed: true,
            version: manifest.version.clone(),
            digest,
            granted,
        };
        core.store.state.put(id.clone(), entry);
        core.store.save()?;
        core.write_rights()?;
        self.note("launcher.installed", &id);
        Ok(InstallReply {
            app: id.to_string(),
            uid,
            profile: profile_name.word().to_owned(),
            declared: states(manifest.declared, granted),
        })
    }

    /// What `app` declared and was granted.
    pub fn permissions(&self, app: &str) -> Result<PermissionsReply, LaunchError> {
        let id = app_id(app)?;
        let core = lock(&self.core);
        let entry = core.installed(&id)?;
        let manifest = core.manifest(&id)?;
        Ok(PermissionsReply {
            declared: states(manifest.declared, entry.granted),
            bus: manifest.bus,
            files: manifest.files,
        })
    }

    /// Grants (`grant`) or takes back a declared permission of `app`.
    /// Taking one back stops the app's running instances (a cell keeps
    /// what it was started with).
    pub fn set_permission(
        &self,
        app: &str,
        name: &str,
        grant: bool,
    ) -> Result<PermissionReply, LaunchError> {
        let id = app_id(app)?;
        let permission = Permission::from_name(name).ok_or(LaunchError::NotDeclared)?;
        let mut core = lock(&self.core);
        let entry = core.installed(&id)?.clone();
        let manifest = core.manifest(&id)?;
        if !manifest.declared.has(permission) {
            return Err(LaunchError::NotDeclared);
        }
        if !entry.profile.profile().grantable.has(permission) {
            return Err(LaunchError::NotGrantable);
        }
        let was = entry.granted.has(permission);
        let granted = if grant {
            entry.granted.with(permission)
        } else {
            entry.granted.without(permission)
        };
        if let Some(e) = core.store.state.get_mut(&id) {
            e.granted = granted;
        }
        core.store.save()?;
        core.write_rights()?;
        let stopped = if was && !grant {
            self.stop_all_locked(&mut core, &id, false)
        } else {
            0
        };
        self.note(
            if grant {
                "launcher.granted"
            } else {
                "launcher.revoked"
            },
            &id,
        );
        Ok(PermissionReply {
            granted: grant,
            stopped,
        })
    }

    /// Removes `app`: ends it, removes its data (unless `keep_data`); its
    /// user id stays reserved for it. Returns the instances ended.
    pub fn uninstall(&self, app: &str, keep_data: bool) -> Result<u32, LaunchError> {
        let id = app_id(app)?;
        let mut core = lock(&self.core);
        core.installed(&id)?;
        let stopped = self.stop_all_locked(&mut core, &id, true);
        if !keep_data {
            // before the entry changes: a failure leaves the app installed
            datadir::remove_app_dir(&core.config.paths.data_root, id.as_str())
                .map_err(|_| LaunchError::Storage(StateError::Io(std::io::ErrorKind::Other)))?;
        }
        core.store.remove_manifest(&id)?;
        if let Some(e) = core.store.state.get_mut(&id) {
            e.installed = false;
            e.granted = PermissionSet::EMPTY;
        }
        core.store.save()?;
        core.write_rights()?;
        if let Some(root) = &core.config.paths.group_root {
            ultar_init::groups::remove(&root.join(Slice::Apps.dir()).join(id.as_str()));
        }
        self.note("launcher.uninstalled", &id);
        Ok(stopped)
    }
}
