//! A scripted backend for tests and screen shots: answers from data, every
//! command recorded. No file, no socket, no network — the tests never touch
//! the system, and no success is ever reported that the script did not say.

use crate::action::{Command, StoreEvent};
use crate::backend::{event_catalog, io_error, Backend};
use crate::error::{BackendError, SourceError, StoreError};
use crate::source::{FoundSource, LoadedCatalog};
use crate::state::{FreeEntry, InstalledApp};
use std::sync::Mutex;

/// The scripted answers behind one mutex (the shell may call from its worker).
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

/// A scripted backend for tests and screen shots.
pub struct ScriptedBackend {
    state: Mutex<Scripted>,
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
}
