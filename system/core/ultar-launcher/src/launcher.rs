//! The launcher: installed apps, their grants and their running instances,
//! behind one lock. Starting and watching cells is in [`crate::instances`],
//! the cell itself in [`crate::cell`].

use crate::appid::AppId;
use crate::config::Config;
use crate::datadir::{self, Owner};
use crate::error::{LaunchError, PackageError};
use crate::instances::Instance;
use crate::manifest::Manifest;
use crate::perm::{Permission, PermissionSet};
use crate::platform::{Feature, Platform};
use crate::rightsfile;
use crate::store::{Entry, StateError, Store};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use ultar_bus::messages::launcher::{PermissionReply, PermissionState, PermissionsReply, Role};
use ultar_init::output::Forwarder;
use ultar_init::spec::Slice;
use ultar_service::log::{Level, Logger, Record};

/// State behind the lock.
pub(crate) struct Core {
    pub(crate) config: Config,
    pub(crate) platform: Platform,
    pub(crate) store: Store,
    pub(crate) instances: BTreeMap<(AppId, Role), Instance>,
    pub(crate) generation: u64,
    pub(crate) stopping: bool,
    /// Apps being uninstalled (no start meanwhile).
    pub(crate) removing: std::collections::BTreeSet<AppId>,
    /// Default apps (`apps.default.*`), read at start and on `settings.changed`.
    pub(crate) defaults: crate::defaults::Defaults,
    /// The autostart list and what was started this session.
    pub(crate) autostart: crate::defaults::Autostart,
    /// The downloads clean-up (`storage.clean_downloads`, `_days`) and when it last ran.
    pub(crate) clean: crate::clean::Clean,
    /// The person's look, handed to every app it starts (`crate::look`).
    pub(crate) look: crate::look::Look,
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

pub(crate) fn states(declared: PermissionSet, granted: PermissionSet) -> Vec<PermissionState> {
    declared
        .iter()
        .map(|p| PermissionState {
            name: p.name().to_owned(),
            granted: granted.has(p),
        })
        .collect()
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
        let foreign = self
            .store
            .state
            .get(id)
            .is_some_and(|e| e.profile == crate::profile::ProfileName::Foreign);
        let parsed = if foreign {
            crate::foreign::parse(&bytes)
        } else {
            Manifest::parse(&bytes)
        };
        parsed.map_err(|e| LaunchError::Package(PackageError::Manifest(e)))
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

    pub(crate) fn set_granted(&mut self, id: &AppId, granted: PermissionSet) {
        if let Some(e) = self.store.state.get_mut(id) {
            e.granted = granted;
        }
    }

    /// Writes the compositor's app rights file; if that fails, removes it
    /// (the compositor then gives no app a right) and reports the error.
    pub(crate) fn write_rights_or_withdraw(&self) -> Result<(), LaunchError> {
        let written = self.write_rights();
        if written.is_err() {
            if let Some(path) = &self.config.paths.compositor_rights {
                rightsfile::withdraw(path);
            }
            if let Some(path) = &self.config.paths.bus_apps {
                rightsfile::withdraw(path);
            }
        }
        written
    }

    /// Writes the compositor's app rights file and the broker's app map.
    pub(crate) fn write_rights(&self) -> Result<(), LaunchError> {
        self.write_bus_map()?;
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

impl Core {
    /// Whether the installed app `id` reaches the bus: its signed manifest
    /// declares bus calls and the apps' socket is configured.
    pub(crate) fn declares_bus(&self, manifest: &Manifest) -> bool {
        self.config.paths.app_bus.is_some() && !manifest.bus.is_empty()
    }

    /// Writes the broker's map of app user ids: one line per installed app
    /// whose signed manifest declares bus access (an app without
    /// declarations has no bus identity: the broker refuses it).
    fn write_bus_map(&self) -> Result<(), LaunchError> {
        let Some(path) = &self.config.paths.bus_apps else {
            return Ok(());
        };
        let apps: Vec<(u32, String)> = self
            .store
            .state
            .apps()
            .filter(|(_, e)| e.installed)
            .filter_map(|(id, e)| {
                let manifest = self.manifest(id).ok()?;
                self.declares_bus(&manifest)
                    .then(|| (e.uid, id.to_string()))
            })
            .collect();
        rightsfile::write_bus(path, &apps).map_err(|e| LaunchError::Storage(e.into()))
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
        if let (Some(root), Some(group)) = (&config.paths.areas_root, config.areas_group) {
            crate::areas::ensure(root, group);
        }
        if let Some(root) = &config.paths.publish_root {
            // not made: no app gets a publish directory (`plan::publish_dir`), nothing else fails
            let _ = datadir::ensure_public_dir(root);
        }
        let store = Store::open(&config.paths.state_dir, config.uids)?;
        // cells of a previous run (by their memory groups): no second copy
        if let (Some(root), true) = (&config.paths.group_root, platform.groups) {
            for id in crate::cleanup::end_leftovers(root, store.state.apps().map(|(id, _)| id)) {
                logger.emit(
                    Record::new(Level::Error, "launcher.leftover_not_ended")
                        .id_or_invalid("app", id.as_str()),
                );
            }
        }
        let output = Forwarder::new(config.paths.output_socket.clone());
        let core = Core {
            config,
            platform,
            store,
            instances: BTreeMap::new(),
            generation: 0,
            stopping: false,
            removing: std::collections::BTreeSet::new(),
            defaults: crate::defaults::Defaults::default(),
            autostart: crate::defaults::Autostart::default(),
            clean: crate::clean::Clean::default(),
            look: crate::look::Look::default(),
        };
        core.write_rights_or_withdraw()?;
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
    /// Taking one back first stops the app's running instances (a cell and
    /// a compositor connection keep what they were started with), then
    /// stores the change; if the compositor's file cannot be written it is
    /// withdrawn (no app has a `ui.*` right until the next good write) and
    /// the error is reported – fail closed. A grant that cannot reach the
    /// compositor is taken back.
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
        let revoking = entry.granted.has(permission) && !grant;
        let stopped = if revoking {
            self.stop_all_locked(&mut core, &id, false)
        } else {
            0
        };
        let granted = if grant {
            entry.granted.with(permission)
        } else {
            entry.granted.without(permission)
        };
        core.set_granted(&id, granted);
        // the compositor's file follows the state in memory even when the
        // state cannot be saved (review round 2: a revoke whose save failed
        // left the right on file for the app's next start)
        let saved = core.store.save().map_err(LaunchError::from);
        let written = core.write_rights_or_withdraw();
        if let Err(error) = saved.and(written) {
            if grant {
                core.set_granted(&id, entry.granted);
                let _ = core.store.save();
                let _ = core.write_rights_or_withdraw();
            }
            self.note("launcher.permission_not_stored", &id);
            return Err(error);
        }
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

    /// Removes `app`: ends it and waits (bounded) until its cells are gone,
    /// then removes its data (unless `keep_data`); its user id stays
    /// reserved for it and its signer. Returns the instances ended.
    pub fn uninstall(&self, app: &str, keep_data: bool) -> Result<u32, LaunchError> {
        let id = app_id(app)?;
        let mut core = lock(&self.core);
        core.installed(&id)?;
        if ultar_brand::base_apps::is_base_app(id.as_str()) {
            return Err(LaunchError::Protected);
        }
        if !core.removing.insert(id.clone()) {
            return Err(LaunchError::Busy);
        }
        let stopped = self.stop_all_locked(&mut core, &id, true);
        let wait = core.config.stop_grace + std::time::Duration::from_secs(5);
        let (mut core, ended) = self.wait_ended(core, &id, wait);
        let result = if ended {
            self.remove_locked(&mut core, &id, keep_data)
        } else {
            Err(LaunchError::Busy)
        };
        core.removing.remove(&id);
        result.map(|()| stopped)
    }

    fn remove_locked(
        &self,
        core: &mut Core,
        id: &AppId,
        keep_data: bool,
    ) -> Result<(), LaunchError> {
        if !keep_data {
            // before the entry changes: a failure leaves the app installed
            datadir::remove_app_dir(&core.config.paths.data_root, id.as_str())
                .map_err(|_| LaunchError::Storage(StateError::Io(std::io::ErrorKind::Other)))?;
        }
        // what it published goes with it (a cache: the readers fall back to nothing)
        if let Some(root) = &core.config.paths.publish_root {
            let _ = datadir::remove_app_dir(root, id.as_str());
        }
        core.store.remove_manifest(id)?;
        if let Some(e) = core.store.state.get_mut(id) {
            e.installed = false;
            e.granted = PermissionSet::EMPTY;
        }
        core.store.save()?;
        core.write_rights_or_withdraw()?;
        if let Some(root) = &core.config.paths.group_root {
            crate::cleanup::remove_group(&root.join(Slice::Apps.dir()).join(id.as_str()));
        }
        self.note("launcher.uninstalled", id);
        Ok(())
    }
}
