//! The only place the Store talks to the rest of the system (APP-RULES §2
//! `service.rs`): catalogue sources on disk and the `launcher.*` calls over
//! the bus. Everything is behind traits so tests never touch files or
//! sockets; this crate contains no network code at all (offline first).

use crate::action::{Command, StoreEvent};
use crate::error::{BackendError, SourceError, StoreError};
use crate::source::LoadedCatalog;
#[cfg(feature = "bus")]
use crate::source::{find_sources, trusted_dir_from_env, CatalogSource, FileCatalogSource};
#[cfg(feature = "bus")]
use crate::state::InstalledApp;
#[cfg(feature = "bus")]
use std::path::PathBuf;
#[cfg(feature = "bus")]
use std::sync::{Arc, Mutex};

/// The test double that answers from a script (no files, no sockets).
pub use crate::scripted::ScriptedBackend;

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
use ultar_bus::{BusError, ErrorCode};

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
            log_dir: crate::data_dir(),
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
            log_dir: crate::data_dir(),
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
            CatalogLocation::Dir(dir) => {
                Arc::new(FileCatalogSource::new(dir, self.trusted_dir.clone()))
            }
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
impl SystemBackend {
    fn run_catalog(&self, cmd: Command, send: &mut dyn FnMut(StoreEvent)) {
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
                            // The launcher contract (v2) reports neither; the
                            // rows show "size unknown" instead of a guess.
                            size: None,
                            last_used_ms: None,
                        })
                        .collect(),
                )),
                Err(_) => {
                    // Without the launcher nothing can be claimed as installed.
                    send(StoreEvent::Installed(Vec::new()));
                }
            },
            _ => {}
        }
    }

    fn run_launcher(&self, cmd: Command, send: &mut dyn FnMut(StoreEvent)) {
        match cmd {
            Command::Launch(app) => {
                let result = self
                    .call(&launcher::Start {
                        app: app.clone(),
                        role: launcher::Role::Ui,
                        activation_token: None,
                        uri: None,
                        new_window: false,
                    })
                    .map(|_| ())
                    .map_err(StoreError::Backend);
                send(StoreEvent::OperationDone { app, result });
            }
            Command::Install { app, path } => {
                // The launcher verifies the package signature (ultar-pkg); a
                // refused package answers ERROR_PACKAGE_REFUSED and nothing
                // is written.
                // The online source downloads the package first and checks it against
                // the digest its signed catalogue pins.
                let result = match self
                    .source_now()
                    .ensure_package(std::path::Path::new(&path))
                {
                    Ok(()) => self
                        .call(&launcher::Install { package: path })
                        .map(|_| ())
                        .map_err(StoreError::Backend),
                    Err(e) => Err(StoreError::Source(e)),
                };
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
            cmd @ (Command::Free { .. } | Command::FreeItems { .. }) => {
                self.run_free_space(cmd, send)
            }
            _ => {}
        }
    }

    /// The launcher contract on main has no `free_items` / `free` methods
    /// yet (docs/integration/ultar-store.md, open points). Freeing space is
    /// therefore answered honestly: the list keeps its board placeholders
    /// (sizes `—`) and a "free" request fails with [`BackendError::NoLauncher`]
    /// – nothing is claimed as freed.
    fn run_free_space(&self, cmd: Command, send: &mut dyn FnMut(StoreEvent)) {
        if let Command::Free { resume, .. } = cmd {
            send(StoreEvent::OperationDone {
                app: resume.unwrap_or_default(),
                result: Err(StoreError::Backend(BackendError::NoLauncher)),
            });
        }
    }

    fn run_local(&self, cmd: Command, send: &mut dyn FnMut(StoreEvent)) {
        match cmd {
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
            _ => {}
        }
    }
}

#[cfg(feature = "bus")]
impl Backend for SystemBackend {
    fn run(&self, cmd: Command, send: &mut dyn FnMut(StoreEvent)) {
        match cmd {
            Command::ReloadCatalog | Command::UseSource(_) | Command::ReloadInstalled => {
                self.run_catalog(cmd, send)
            }
            Command::Launch(_)
            | Command::Install { .. }
            | Command::Uninstall { .. }
            | Command::Free { .. }
            | Command::FreeItems { .. } => self.run_launcher(cmd, send),
            _ => self.run_local(cmd, send),
        }
    }
}

pub(crate) fn io_error(send: &mut dyn FnMut(StoreEvent)) {
    send(StoreEvent::OperationDone {
        app: String::new(),
        result: Err(StoreError::Backend(BackendError::Io)),
    });
}

pub(crate) fn event_catalog(loaded: Result<LoadedCatalog, SourceError>) -> StoreEvent {
    StoreEvent::CatalogLoaded(loaded.map_err(StoreError::Source))
}

/// Maps a bus error to the app's error using the launcher contract's
/// documented error numbers (`ultar-bus::messages::launcher`).
#[cfg(feature = "bus")]
fn bus_error(e: BusError) -> BackendError {
    match e {
        BusError::Remote(ErrorCode::Service(code)) => match code {
            launcher::ERROR_UNKNOWN_APP => BackendError::UnknownApp,
            launcher::ERROR_PACKAGE_REFUSED => BackendError::PackageRefused,
            launcher::ERROR_START_FAILED => BackendError::StartFailed,
            launcher::ERROR_SHUTTING_DOWN => BackendError::ShuttingDown,
            launcher::ERROR_STORAGE => BackendError::Storage,
            launcher::ERROR_INVALID => BackendError::Invalid,
            launcher::ERROR_PROTECTED => BackendError::Protected,
            _ => BackendError::Protocol,
        },
        BusError::Timeout | BusError::NotConnected | BusError::Closed => BackendError::Unreachable,
        BusError::Remote(
            ErrorCode::UnknownService | ErrorCode::UnknownMethod | ErrorCode::ServiceGone,
        ) => BackendError::NoLauncher,
        _ => BackendError::Protocol,
    }
}
