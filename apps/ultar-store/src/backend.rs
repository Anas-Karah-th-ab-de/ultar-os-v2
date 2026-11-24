//! The only place the Store talks to the rest of the system (APP-RULES §2
//! `service.rs`): catalogue sources on disk and the `launcher.*` calls over
//! the bus. Everything is behind traits so tests never touch files or
//! sockets; this crate contains no network code at all (offline first).

use crate::action::{Command, StoreEvent};
use crate::error::{BackendError, SourceError, StoreError};
use crate::source::{FoundSource, LoadedCatalog};
#[cfg(feature = "bus")]
use crate::source::{find_sources, trusted_dir_from_env, CatalogSource, FileCatalogSource};
use crate::state::{FreeEntry, InstalledApp};
#[allow(unused_imports)]
use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(feature = "bus")]
use std::sync::Arc;

/// Runs the commands a [`crate::Store`] produces. The answer comes back as a
/// [`StoreEvent`] through the sender, so nothing here may block the UI thread
/// (the binary runs commands on its worker).
pub trait Backend: Send + Sync {
    /// Execute one command. A command whose work could not be done reports
    /// that honestly as an error event; a success is never claimed for work
    /// that did not happen.
    fn run(&self, cmd: Command, send: &mut dyn FnMut(StoreEvent));
}

#[cfg(feature = "bus")]
use ultar_bus::messages::launcher;
#[cfg(feature = "bus")]
use ultar_bus::BusError;

/// The real backend: catalogues from disk (built-in, USB stick, folder) and
/// package operations through the `launcher` service on the bus
/// (`system/core/ultar-bus/src/messages/launcher.rs`).
#[cfg(feature = "bus")]
pub struct SystemBackend {
    default_source: Arc<dyn CatalogSource>,
    location: Mutex<CatalogLocation>,
    bus: Option<ultar_bus::Client>,
    media_dir: PathBuf,
    log_dir: Option<PathBuf>,
    trusted_dir: PathBuf,
}

#[cfg(feature = "bus")]
#[derive(Clone)]
enum CatalogLocation {
    BuiltIn,
    Dir(PathBuf),
}

#[cfg(feature = "bus")]
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(feature = "bus")]
impl SystemBackend {
    /// A backend reading the built-in `source`; operations go over the bus
    /// socket when one is given. `None` socket: browsing works, operations
    /// honestly answer [`BackendError::NoLauncher`].
    pub fn new(
        source: Arc<dyn CatalogSource>,
        socket: Option<PathBuf>,
    ) -> Result<SystemBackend, BusError> {
        let bus = match &socket {
            Some(path) => Some(ultar_bus::Client::connect(
                path.as_path(),
                ultar_bus::ClientOptions::named("app.store"),
            )?),
            None => None,
        };
        Ok(SystemBackend {
            default_source: source,
            location: Mutex::new(CatalogLocation::BuiltIn),
            bus,
            media_dir: PathBuf::from("/media"),
            log_dir: std::env::var("ULTAR_STORE_DATA").ok().map(PathBuf::from),
            trusted_dir: trusted_dir_from_env(),
        })
    }

    /// A backend without any bus connection: catalogue browsing only.
    pub fn offline(source: Arc<dyn CatalogSource>) -> SystemBackend {
        SystemBackend {
            default_source: source,
            location: Mutex::new(CatalogLocation::BuiltIn),
            bus: None,
            media_dir: PathBuf::from("/media"),
            log_dir: std::env::var("ULTAR_STORE_DATA").ok().map(PathBuf::from),
            trusted_dir: trusted_dir_from_env(),
        }
    }

    /// Where removable media is looked for (`ULTAR_MEDIA_DIR`).
    pub fn with_media_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.media_dir = dir.into();
        self
    }

    /// Where `store.log` writes (the app's named data area).
    pub fn with_log_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.log_dir = Some(dir.into());
        self
    }

    fn source_now(&self) -> Arc<dyn CatalogSource> {
        let location = match self.location.lock() {
            Ok(loc) => loc.clone(),
            Err(poison) => poison.into_inner().clone(),
        };
        match location {
            CatalogLocation::Dir(dir) => Arc::new(FileCatalogSource::new(
                dir,
                self.trusted_dir.clone(),
            )),
            CatalogLocation::BuiltIn => Arc::clone(&self.default_source),
        }
    }

    fn call<C>(&self, msg: &C) -> Result<C::Reply, BackendError>
    where
        C: ultar_bus::Call,
    {
        let bus = self.bus.as_ref().ok_or(BackendError::NoLauncher)?;
        bus.call(msg, CALL_TIMEOUT).map_err(bus_error)
    }
}

#[cfg(feature = "bus")]
impl Backend for SystemBackend {
    fn run(&self, cmd: Command, send: &mut dyn FnMut(StoreEvent)) {
        match cmd {
            Command::ReloadCatalog => send(event_catalog(self.source_now().load())),
            Command::UseSource(dir) => {
                if let Ok(mut loc) = self.location.lock() {
                    *loc = CatalogLocation::Dir(PathBuf::from(&dir));
                }
                send(event_catalog(self.source_now().load()));
            }
            Command::ReloadInstalled => match self.call(&launcher::List {}) {
                Ok(reply) => send(StoreEvent::Installed(
                    reply
                        .apps
                        .into_iter()
                        .map(|a| InstalledApp {
                            id: a.app,
                            version: a.version,
                            size: a.size,
                            last_used_ms: a.last_used_ms,
                        })
                        .collect(),
                )),
                Err(_) => {
                    // Without the launcher nothing can be claimed as installed.
                    send(StoreEvent::Installed(Vec::new()));
                }
            },
            Command::Launch(app) => {
                let result = self
                    .call(&launcher::Start {
                        app: app.clone(),
                        role: launcher::Role::Ui,
                        activation_token: None,
                    })
                    .map(|_| ())
                    .map_err(StoreError::Backend);
                send(StoreEvent::OperationDone { app, result });
            }
            Command::Install { app, path } => {
                // The launcher verifies the package signature (ultar-pkg); a
                // refused package answers ERROR_PACKAGE_REFUSED and nothing
                // is written.
                let result = self
                    .call(&launcher::Install { package: path })
                    .map(|_| ())
                    .map_err(StoreError::Backend);
                send(StoreEvent::OperationDone { app, result });
            }
            Command::Uninstall { app, keep_data } => {
                match self.call(&launcher::Uninstall {
                    app: app.clone(),
                    keep_data,
                }) {
                    Ok(_) => send(StoreEvent::Removed(app)),
                    Err(e) => send(StoreEvent::OperationDone {
                        app,
                        result: Err(StoreError::Backend(e)),
                    }),
                }
            }
            Command::Free { ids, resume } => {
                match self.call(&launcher::Free {
                    ids,
                    resume: resume.clone(),
                }) {
                    Ok(reply) => send(StoreEvent::Freed {
                        freed: reply.freed,
                        resumed: if reply.install_done { resume } else { None },
                    }),
                    Err(e) => send(StoreEvent::OperationDone {
                        app: resume.unwrap_or_default(),
                        result: Err(StoreError::Backend(e)),
                    }),
                }
            }
            Command::FreeItems { for_app } => {
                if let Ok(reply) = self.call(&launcher::FreeItems {
                    for_app: for_app.clone(),
                }) {
                    send(StoreEvent::FreeItems(
                        reply
                            .items
                            .into_iter()
                            .map(|i| FreeEntry {
                                id: i.id,
                                size: i.size,
                                selected: true,
                            })
                            .collect(),
                        reply.required,
                        reply.available,
                    ));
                }
                // No answer: the model's placeholder list (sizes `—`, exactly
                // as the board defines them) stays.
            }
            Command::SaveLog { report } => match &self.log_dir {
                Some(dir) => {
                    let path = dir.join("store-error-report.txt");
                    if std::fs::create_dir_all(dir).is_ok() && std::fs::write(&path, report).is_ok()
                    {
                        send(StoreEvent::LogSaved {
                            path: path.display().to_string(),
                        });
                    } else {
                        io_error(send);
                    }
                }
                None => io_error(send),
            },
            Command::DiscoverSources => {
                let found = find_sources(&self.media_dir);
                send(StoreEvent::Sources(found));
            }
            // The removal question is a dialog; cancel and a missing package
            // file are answered by the model itself.
            Command::AskRemove(_) | Command::CancelOp { .. } | Command::PackageMissing(_) => {}
        }
    }
}

fn io_error(send: &mut dyn FnMut(StoreEvent)) {
    send(StoreEvent::OperationDone {
        app: String::new(),
        result: Err(StoreError::Backend(BackendError::Io)),
    });
}

fn event_catalog(loaded: Result<LoadedCatalog, SourceError>) -> StoreEvent {
    StoreEvent::CatalogLoaded(loaded.map_err(StoreError::Source))
}

/// Maps a bus error to the app's error using the launcher contract's
/// documented error numbers (`ultar-bus::messages::launcher`).
#[cfg(feature = "bus")]
fn bus_error(e: BusError) -> BackendError {
    match e {
        BusError::Service(code) => match code {
            launcher::ERROR_UNKNOWN_APP => BackendError::UnknownApp,
            launcher::ERROR_PACKAGE_REFUSED => BackendError::PackageRefused,
            launcher::ERROR_START_FAILED => BackendError::StartFailed,
            launcher::ERROR_SHUTTING_DOWN => BackendError::ShuttingDown,
            launcher::ERROR_STORAGE => BackendError::Storage,
            launcher::ERROR_NO_SPACE => BackendError::NoSpace {
                required: None,
                available: None,
            },
            launcher::ERROR_INVALID => BackendError::Invalid,
            _ => BackendError::Protocol,
        },
        BusError::Timeout | BusError::NotConnected | BusError::Closed => BackendError::Unreachable,
        BusError::UnknownService | BusError::UnknownMethod => BackendError::NoLauncher,
        _ => BackendError::Protocol,
    }
}

/// A scripted backend for tests and screen shots: answers from data, every
/// command recorded. No file, no socket, no network.
pub struct ScriptedBackend {
    state: Mutex<Scripted>,
}

#[derive(Default)]
struct Scripted {
    catalog: Option<Result<LoadedCatalog, SourceError>>,
    installed: Vec<InstalledApp>,
    sources: Vec<FoundSource>,
    install_results: std::collections::BTreeMap<String, Result<(), StoreError>>,
    free_items: Vec<FreeEntry>,
    required: Option<u64>,
    available: Option<u64>,
    freed: u64,
    install_done_after_free: bool,
    seen: Vec<Command>,
    launch_error: Option<BackendError>,
    log_error: bool,
}

impl ScriptedBackend {
    /// Answers `ReloadCatalog` with this catalogue (or failure).
    pub fn with_catalog(catalog: Result<LoadedCatalog, SourceError>) -> ScriptedBackend {
        ScriptedBackend {
            state: Mutex::new(Scripted {
                catalog: Some(catalog),
                ..Scripted::default()
            }),
        }
    }

    /// A backend with nothing loaded (every load answers `Unreadable`).
    pub fn empty() -> ScriptedBackend {
        ScriptedBackend {
            state: Mutex::new(Scripted::default()),
        }
    }

    fn edit<R>(&self, f: impl FnOnce(&mut Scripted) -> R) -> R {
        match self.state.lock() {
            Ok(mut g) => f(&mut g),
            Err(poison) => {
                let mut inner = poison.into_inner();
                f(&mut inner)
            }
        }
    }

    /// The install of `app` answers `result`.
    pub fn installs(&self, app: &str, result: Result<(), StoreError>) {
        self.edit(|s| {
            s.install_results.insert(app.to_string(), result);
        });
    }

    /// The installed list.
    pub fn set_installed(&self, list: Vec<InstalledApp>) {
        self.edit(|s| s.installed = list);
    }

    /// The free-space list with required/available.
    pub fn set_free(
        &self,
        items: Vec<FreeEntry>,
        required: Option<u64>,
        available: Option<u64>,
    ) {
        self.edit(|s| {
            s.free_items = items;
            s.required = required;
            s.available = available;
        });
    }

    /// `Free` reports this many bytes freed; `install_done` continues the install.
    pub fn set_freed(&self, bytes: u64, install_done: bool) {
        self.edit(|s| {
            s.freed = bytes;
            s.install_done_after_free = install_done;
        });
    }

    /// Fail every `Launch` with this error.
    pub fn set_launch_error(&self, error: BackendError) {
        self.edit(|s| s.launch_error = Some(error));
    }

    /// The found offline sources.
    pub fn set_sources(&self, sources: Vec<FoundSource>) {
        self.edit(|s| s.sources = sources);
    }

    /// Make `SaveLog` fail.
    pub fn set_log_error(&self) {
        self.edit(|s| s.log_error = true);
    }

    /// Every command seen, in order.
    pub fn seen(&self) -> Vec<Command> {
        self.edit(|s| s.seen.clone())
    }

    /// The last command seen.
    pub fn last_command(&self) -> Option<Command> {
        self.edit(|s| s.seen.last().cloned())
    }
}

impl Backend for ScriptedBackend {
    fn run(&self, cmd: Command, send: &mut dyn FnMut(StoreEvent)) {
        let snap = self.edit(|s| {
            s.seen.push(cmd.clone());
            Snapshot::of(s, &cmd)
        });
        match cmd {
            Command::ReloadCatalog | Command::UseSource(_) => match snap.catalog {
                Some(loaded) => send(event_catalog(loaded)),
                None => send(event_catalog(Err(SourceError::Unreadable))),
            },
            Command::ReloadInstalled => send(StoreEvent::Installed(snap.installed)),
            Command::DiscoverSources => send(StoreEvent::Sources(snap.sources)),
            Command::Launch(app) => {
                let result = match snap.launch_error {
                    Some(e) => Err(StoreError::Backend(e)),
                    None => Ok(()),
                };
                send(StoreEvent::OperationDone { app, result });
            }
            Command::Install { app, .. } => {
                let result = snap.install.unwrap_or(Ok(()));
                send(StoreEvent::OperationDone { app, result });
            }
            Command::Uninstall { app, .. } => send(StoreEvent::Removed(app)),
            Command::Free { resume, .. } => send(StoreEvent::Freed {
                freed: snap.freed,
                resumed: if snap.install_done_after_free {
                    resume
                } else {
                    None
                },
            }),
            Command::FreeItems { .. } => {
                send(StoreEvent::FreeItems(snap.free_items, snap.required, snap.available))
            }
            Command::SaveLog { .. } => {
                if snap.log_error {
                    io_error(send);
                } else {
                    send(StoreEvent::LogSaved {
                        path: "/data/ultar/apps/store/log/store-error-report.txt".into(),
                    });
                }
            }
            Command::AskRemove(_) | Command::CancelOp { .. } | Command::PackageMissing(_) => {}
        }
    }
}

/// The scripted answers needed for one command (cloned out of the lock).
struct Snapshot {
    catalog: Option<Result<LoadedCatalog, SourceError>>,
    installed: Vec<InstalledApp>,
    sources: Vec<FoundSource>,
    install: Option<Result<(), StoreError>>,
    free_items: Vec<FreeEntry>,
    required: Option<u64>,
    available: Option<u64>,
    freed: u64,
    install_done_after_free: bool,
    launch_error: Option<BackendError>,
    log_error: bool,
}

impl Snapshot {
    fn of(s: &Scripted, cmd: &Command) -> Snapshot {
        Snapshot {
            catalog: s.catalog.clone(),
            installed: s.installed.clone(),
            sources: s.sources.clone(),
            install: match cmd {
                Command::Install { app, .. } => s.install_results.get(app).cloned(),
                _ => None,
            },
            free_items: s.free_items.clone(),
            required: s.required,
            available: s.available,
            freed: s.freed,
            install_done_after_free: s.install_done_after_free,
            launch_error: s.launch_error.clone(),
            log_error: s.log_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, CatalogApp, Loc, Source};
    use crate::error::{BackendError, SourceError};

    fn loc(s: &str) -> Loc {
        Loc {
            ar: s.into(),
            en: s.into(),
            de: None,
        }
    }

    fn sample_catalog() -> Catalog {
        Catalog {
            format_version: crate::catalog::FORMAT_VERSION,
            generated: Some("2026-11-24".into()),
            apps: vec![CatalogApp {
                id: "km-office".into(),
                name: loc("الأدوات المكتبية"),
                summary: loc("مستندات وجداول"),
                description: loc("وصف"),
                publisher: "KM Code Craft".into(),
                category: "office".into(),
                icon: "office".into(),
                source: Source::Ultar,
                version: Some("1.4.0".into()),
                size_download: None,
                size_installed: None,
                file_types: vec![".docx".into(), ".xlsx".into()],
                permissions: Vec::new(),
                requires: Vec::new(),
                package: Some("km-office.uap".into()),
            }],
        }
    }

    fn loaded() -> LoadedCatalog {
        LoadedCatalog {
            catalog: sample_catalog(),
            verified: true,
            signer: Some("k".into()),
            dir: Some("/stick".into()),
        }
    }

    #[test]
    fn scripted_backend_answers_and_records() {
        let be = ScriptedBackend::with_catalog(Ok(loaded()));
        let mut events = Vec::new();
        be.run(Command::ReloadCatalog, &mut |e| events.push(e));
        assert_eq!(events.len(), 1);
        match &events[0] {
            StoreEvent::CatalogLoaded(Ok(l)) => {
                assert_eq!(l.catalog.apps.len(), 1);
                assert_eq!(l.dir.as_deref(), Some("/stick"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(be.seen(), vec![Command::ReloadCatalog]);
    }

    #[test]
    fn scripted_backend_reports_failures_honestly() {
        let be = ScriptedBackend::empty();
        be.installs(
            "media",
            Err(StoreError::Backend(BackendError::PackageRefused)),
        );
        be.set_launch_error(BackendError::StartFailed);
        be.set_log_error();
        let mut events = Vec::new();
        be.run(
            Command::Install {
                app: "media".into(),
                path: "/x/media.uap".into(),
            },
            &mut |e| events.push(e),
        );
        be.run(Command::Launch("media".into()), &mut |e| events.push(e));
        be.run(
            Command::SaveLog {
                report: "r".into(),
            },
            &mut |e| events.push(e),
        );
        assert!(matches!(
            events[0],
            StoreEvent::OperationDone {
                result: Err(StoreError::Backend(BackendError::PackageRefused)),
                ..
            }
        ));
        assert!(matches!(
            events[1],
            StoreEvent::OperationDone {
                result: Err(StoreError::Backend(BackendError::StartFailed)),
                ..
            }
        ));
        assert!(matches!(
            events[2],
            StoreEvent::OperationDone {
                result: Err(StoreError::Backend(BackendError::Io)),
                ..
            }
        ));
    }

    #[test]
    fn catalog_only_backend_without_source_answers_offline() {
        let be = ScriptedBackend::empty();
        let mut events = Vec::new();
        be.run(Command::ReloadCatalog, &mut |e| events.push(e));
        assert!(matches!(
            events[0],
            StoreEvent::CatalogLoaded(Err(StoreError::Source(SourceError::Unreadable)))
        ));
    }

    #[cfg(feature = "bus")]
    #[test]
    fn bus_error_mapping_follows_the_contract() {
        assert_eq!(
            bus_error(BusError::Service(launcher::ERROR_PACKAGE_REFUSED)),
            BackendError::PackageRefused
        );
        assert!(matches!(
            bus_error(BusError::Service(launcher::ERROR_NO_SPACE)),
            BackendError::NoSpace { .. }
        ));
        assert_eq!(
            bus_error(BusError::UnknownService),
            BackendError::NoLauncher
        );
        assert_eq!(bus_error(BusError::Timeout), BackendError::Unreachable);
    }
}
