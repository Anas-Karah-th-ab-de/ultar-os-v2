//! Clicks and answers → state + commands (APP-RULES §2 `update.rs`): pure
//! logic, testable without a screen. The view calls [`Store::act`] for every
//! user action; the shell feeds [`StoreEvent`] answers back. Nothing here
//! touches I/O.

use crate::action::{Command, StoreAction, StoreEvent};
use crate::error::{BackendError, StoreError};
use crate::state::{
    Area, FreeEntry, FreeSpace, OpKind, OpState, Operation, Store, Surface,
};

impl Store {
    /// Handles one user action; returns the commands the shell must run.
    pub fn act(&mut self, action: StoreAction) -> Vec<Command> {
        match action {
            StoreAction::Area(area) => {
                self.query.clear();
                self.category = None;
                self.area = area;
                self.open_app = None;
                self.back_to = None;
                match area {
                    Area::Installed | Area::Updates => vec![Command::ReloadInstalled],
                    Area::OfflineSources => vec![Command::DiscoverSources],
                    _ => Vec::new(),
                }
            }
            StoreAction::Category(cat) => {
                self.query.clear();
                self.category = Some(cat);
                self.area = Area::Categories;
                self.open_app = None;
                Vec::new()
            }
            StoreAction::Search(q) => {
                self.query = q;
                if self.query.is_empty() {
                    if self.area == Area::Search {
                        self.area = Area::Discover;
                    }
                } else {
                    self.area = Area::Search;
                }
                Vec::new()
            }
            StoreAction::SearchClear => {
                self.query.clear();
                self.area = Area::Discover;
                Vec::new()
            }
            StoreAction::More => Vec::new(),
            StoreAction::Open(app) => {
                if self.area != Area::AppPage {
                    self.back_to = Some(if self.area == Area::Search && !self.query.is_empty() {
                        Area::Search
                    } else {
                        self.area
                    });
                    self.open_app = Some(app);
                    self.area = Area::AppPage;
                }
                Vec::new()
            }
            StoreAction::Back => {
                // Returns to the list the page was opened from; the list is
                // still there with the same query (CLICKS `store.back`).
                self.area = self.back_to.take().unwrap_or(Area::Discover);
                self.open_app = None;
                Vec::new()
            }
            StoreAction::Launch(app) => vec![Command::Launch(app)],
            StoreAction::Install(app) => {
                let kind = if self.is_installed(&app) {
                    OpKind::Update
                } else {
                    OpKind::Install
                };
                self.begin_op(kind, &app)
            }
            StoreAction::UpdateAll => {
                self.update_queue = self
                    .apps()
                    .iter()
                    .filter(|a| self.update_available(&a.id))
                    .map(|a| a.id.clone())
                    .collect();
                self.next_in_queue()
            }
            StoreAction::Remove(app) => vec![Command::AskRemove(app)],
            StoreAction::RemoveConfirmed { app, keep_data } => {
                self.operation = Some(Operation::started(OpKind::Remove, &app));
                vec![Command::Uninstall { app, keep_data }]
            }
            StoreAction::RemoveCancelled => Vec::new(),
            StoreAction::Cancel => match self.operation.take() {
                Some(op) => {
                    self.free.entries.clear();
                    self.update_queue.clear();
                    vec![Command::CancelOp { kind: op.kind }]
                }
                None => Vec::new(),
            },
            StoreAction::Retry => vec![Command::ReloadCatalog],
            StoreAction::Log => match &self.last_error {
                Some(e) => vec![Command::SaveLog {
                    report: self.report_text(e),
                }],
                None => Vec::new(),
            },
            StoreAction::Offline => {
                self.area = Area::OfflineSources;
                self.open_app = None;
                vec![Command::DiscoverSources]
            }
            StoreAction::UseSource(dir) => vec![Command::UseSource(dir)],
            StoreAction::FreeItem(id) => {
                if let Some(f) = self.free.entries.iter_mut().find(|f| f.id == id) {
                    f.selected = !f.selected;
                }
                Vec::new()
            }
            StoreAction::FreeGo => {
                let ids: Vec<String> = self
                    .free
                    .entries
                    .iter()
                    .filter(|f| f.selected)
                    .map(|f| f.id.clone())
                    .collect();
                let resume = self
                    .operation
                    .as_ref()
                    .and_then(|o| o.no_space_for.clone());
                vec![Command::Free { ids, resume }]
            }
            StoreAction::FreeCancel => {
                // Cancels the installation; nothing is freed (CLICKS).
                let kind = self.operation.as_ref().map(|o| o.kind).unwrap_or(OpKind::Install);
                self.operation = None;
                self.free.entries.clear();
                vec![Command::CancelOp { kind }]
            }
            StoreAction::InstallFile(path) => {
                self.operation = Some(Operation::started(OpKind::Install, &path));
                vec![Command::Install {
                    app: path.clone(),
                    path,
                }]
            }
        }
    }

    /// Starts an install/update if the catalogue carries the package file and
    /// every requirement is installed.
    fn begin_op(&mut self, kind: OpKind, app: &str) -> Vec<Command> {
        let Some(entry) = self.entry(app) else {
            return Vec::new();
        };
        let missing = entry
            .requires
            .iter()
            .any(|r| !self.is_installed(r));
        if missing {
            // The requirement is named on the app page; installing it is the
            // user's click there. Nothing starts yet.
            return Vec::new();
        }
        match self.package_path(app) {
            Some(path) => {
                self.operation = Some(Operation::started(kind, app));
                vec![Command::Install {
                    app: app.to_string(),
                    path,
                }]
            }
            None => vec![Command::PackageMissing(app.to_string())],
        }
    }

    /// Continues the "update all" chain; empty when it is done.
    fn next_in_queue(&mut self) -> Vec<Command> {
        match self.update_queue.first() {
            Some(app) => {
                let app = app.clone();
                self.begin_op(OpKind::Update, &app)
            }
            None => Vec::new(),
        }
    }

    /// The package file the catalogue offers for this app (offline first).
    pub fn package_path(&self, app: &str) -> Option<String> {
        let file = self.entry(app)?.package.clone()?;
        let dir = self.catalog.dir.as_deref()?;
        Some(format!("{dir}/{file}"))
    }

    /// Feeds one answer from the shell back into the state.
    pub fn event(&mut self, ev: StoreEvent) -> Vec<Command> {
        match ev {
            StoreEvent::CatalogLoaded(loaded) => self.catalog_loaded(loaded),
            StoreEvent::Installed(list) => {
                self.installed = list;
                Vec::new()
            }
            StoreEvent::Progress { app, percent } => {
                if let Some(op) = self.operation.as_mut() {
                    if let Some(s) = op.states.get_mut(&app) {
                        *s = OpState::Running(percent);
                    }
                }
                Vec::new()
            }
            StoreEvent::OperationDone { app, result } => self.operation_done(app, result),
            StoreEvent::FreeItems(items, required, available) => {
                let previous = std::mem::take(&mut self.free.entries);
                self.free.entries = items
                    .into_iter()
                    .map(|f| {
                        let selected = previous
                            .iter()
                            .find(|s| s.id == f.id)
                            .map(|s| s.selected)
                            .unwrap_or(true);
                        FreeEntry {
                            id: f.id,
                            size: f.size,
                            selected,
                        }
                    })
                    .collect();
                self.free.required = required;
                self.free.available = available;
                Vec::new()
            }
            StoreEvent::Freed { freed, resumed } => {
                let _ = freed;
                match resumed {
                    Some(app) => {
                        self.free.entries.clear();
                        self.begin_op(OpKind::Install, &app)
                    }
                    None => Vec::new(),
                }
            }
            StoreEvent::Sources(found) => {
                self.catalog.sources = found;
                Vec::new()
            }
            StoreEvent::Removed(app) => {
                self.installed.retain(|a| a.id != app);
                if let Some(op) = self.operation.as_mut() {
                    op.states.insert(app, OpState::Done);
                }
                Vec::new()
            }
            StoreEvent::LogSaved { path } => {
                self.log_saved = Some(path);
                Vec::new()
            }
        }
    }

    /// The catalogue load answer: Ready, Offline or Error (never a fake list).
    fn catalog_loaded(
        &mut self,
        loaded: Result<crate::source::LoadedCatalog, StoreError>,
    ) -> Vec<Command> {
        match loaded {
            Ok(cat) => {
                self.catalog.verified = cat.verified;
                self.catalog.dir = cat.dir;
                self.catalog.data = Some(cat.catalog);
                self.surface = Surface::Ready;
                self.last_error = None;
                vec![Command::ReloadInstalled]
            }
            Err(err) => {
                self.last_error = Some(err.clone());
                if err.is_offline() {
                    self.surface = Surface::Offline;
                } else {
                    self.surface = Surface::Error {
                        key: err.message_key().to_string(),
                    };
                }
                // Offline and error surfaces still show what is installed.
                vec![Command::ReloadInstalled]
            }
        }
    }

    /// The outcome of an operation: success, no-space (free list), or reason.
    fn operation_done(&mut self, app: String, result: Result<(), StoreError>) -> Vec<Command> {
        match result {
            Ok(()) => {
                if let Some(op) = self.operation.as_mut() {
                    op.states.insert(app.clone(), OpState::Done);
                }
                self.free = FreeSpace::default();
                self.operation = None;
                // Update-all continues with the next app; otherwise refresh.
                self.update_queue.retain(|a| *a != app);
                if !self.update_queue.is_empty() {
                    return self.next_in_queue();
                }
                vec![Command::ReloadInstalled]
            }
            Err(StoreError::Backend(BackendError::NoSpace {
                required,
                available,
            })) => {
                if let Some(op) = self.operation.as_mut() {
                    op.no_space_for = Some(app.clone());
                    op.states.insert(app.clone(), OpState::Failed);
                }
                self.free.required = required;
                self.free.available = available;
                if self.free.entries.is_empty() {
                    self.free.entries = default_free_entries();
                }
                vec![Command::FreeItems {
                    for_app: Some(app),
                }]
            }
            Err(err) => {
                if let Some(op) = self.operation.as_mut() {
                    op.states.insert(app, OpState::Failed);
                }
                self.last_error = Some(err);
                Vec::new()
            }
        }
    }

    /// The text `store.log` saves: the error with its key and the current surface.
    fn report_text(&self, e: &StoreError) -> String {
        format!(
            "ultar-store error report\nsurface: {:?}\narea: {:?}\nerror: {}\n",
            self.surface, self.area, e
        )
    }
}

/// The board's default list when the launcher has no numbers yet:
/// trash and temp files selected, old updates not (as drawn), all sizes `—`.
fn default_free_entries() -> Vec<FreeEntry> {
    let mut out = Vec::new();
    for (i, id) in ["trash", "temp", "old-updates"].iter().enumerate() {
        out.push(FreeEntry {
            id: (*id).to_string(),
            size: None,
            selected: i < 2,
        });
    }
    out
}

/// The hits for the current query (views and tests).
pub fn visible_results(store: &Store) -> Vec<crate::search::Hit<'_>> {
    if store.query.is_empty() {
        return Vec::new();
    }
    match &store.catalog.data {
        Some(c) => crate::search::search(c, &store.query),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, CatalogApp, Loc, Source};
    use crate::error::SourceError;
    use crate::source::LoadedCatalog;
    use crate::state::InstalledApp;

    fn loc(s: &str) -> Loc {
        Loc {
            ar: s.into(),
            en: s.into(),
            de: None,
        }
    }

    fn entry(id: &str, version: &str, requires: &[&str], package: Option<&str>) -> CatalogApp {
        CatalogApp {
            id: id.into(),
            name: loc(id),
            summary: loc("s"),
            description: loc("d"),
            publisher: "p".into(),
            category: "c".into(),
            icon: "office".into(),
            source: Source::Ultar,
            version: Some(version.into()),
            size_download: None,
            size_installed: None,
            file_types: Vec::new(),
            permissions: Vec::new(),
            requires: requires.iter().map(|s| s.to_string()).collect(),
            package: package.map(|s| s.to_string()),
        }
    }

    fn store_with(ids: &[(&str, &str, &[&str], Option<&str>)]) -> Store {
        let mut s = Store::new();
        s.catalog.data = Some(Catalog {
            format_version: crate::catalog::FORMAT_VERSION,
            generated: None,
            apps: ids
                .iter()
                .map(|(id, v, r, p)| entry(id, v, r, *p))
                .collect(),
        });
        s.catalog.dir = Some("/usb".into());
        s.surface = Surface::Ready;
        s
    }

    #[test]
    fn area_navigation_clears_search_and_opens_lists() {
        let mut s = store_with(&[("a", "1.0", &[], None)]);
        s.act(StoreAction::Search("x".into()));
        assert_eq!(s.area, Area::Search);
        let cmds = s.act(StoreAction::Area(Area::Installed));
        assert_eq!(s.area, Area::Installed);
        assert!(s.query.is_empty());
        assert_eq!(cmds, vec![Command::ReloadInstalled]);
        let cmds = s.act(StoreAction::Area(Area::OfflineSources));
        assert_eq!(cmds, vec![Command::DiscoverSources]);
    }

    #[test]
    fn open_back_returns_to_the_same_list() {
        let mut s = store_with(&[("a", "1.0", &[], None)]);
        s.act(StoreAction::Search("office".into()));
        s.act(StoreAction::Open("a".into()));
        assert_eq!(s.area, Area::AppPage);
        s.act(StoreAction::Back);
        assert_eq!(s.area, Area::Search);
        assert_eq!(s.query, "office");
        assert_eq!(s.open_app, None);
    }

    #[test]
    fn install_needs_a_package_file_or_reports_missing() {
        // A standalone store where only b (no package file) is present.
        let mut s = store_with(&[("b", "1.0", &[], None)]);
        let cmds = s.act(StoreAction::Install("b".into()));
        assert_eq!(cmds, vec![Command::PackageMissing("b".into())]);
        assert!(s.operation.is_none());
        // With a package file it starts and the operation is the install.
        let mut ok = store_with(&[("a", "1.0", &[], Some("a.uap"))]);
        let cmds = ok.act(StoreAction::Install("a".into()));
        assert_eq!(
            cmds,
            vec![Command::Install {
                app: "a".into(),
                path: "/usb/a.uap".into()
            }]
        );
        assert!(ok.operation.is_some());
    }

    #[test]
    fn install_waits_when_a_requirement_is_missing() {
        let mut s = store_with(&[("ff", "1.0", &["linux"], Some("ff.uap"))]);
        let cmds = s.act(StoreAction::Install("ff".into()));
        assert!(cmds.is_empty());
        assert!(s.operation.is_none());
        s.installed.push(InstalledApp {
            id: "linux".into(),
            version: "0.1".into(),
            size: None,
            last_used_ms: None,
        });
        let cmds = s.act(StoreAction::Install("ff".into()));
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn no_space_shows_the_free_list_and_go_continues() {
        let mut s = store_with(&[("media", "1.0", &[], Some("media.uap"))]);
        s.act(StoreAction::Install("media".into()));
        let cmds = s.event(StoreEvent::OperationDone {
            app: "media".into(),
            result: Err(StoreError::Backend(BackendError::NoSpace {
                required: Some(900),
                available: Some(100),
            })),
        });
        assert_eq!(
            cmds,
            vec![Command::FreeItems {
                for_app: Some("media".into())
            }]
        );
        assert_eq!(s.free.required, Some(900));
        assert_eq!(s.free.entries.len(), 3);
        assert!(s.free.entries[0].selected && s.free.entries[1].selected && !s.free.entries[2].selected);
        assert_eq!(s.free.selected_total(), 0); // sizes unknown on the board

        s.act(StoreAction::FreeItem("old-updates".into()));
        assert!(s.free.entries[2].selected);
        let cmds = s.act(StoreAction::FreeGo);
        assert_eq!(
            cmds,
            vec![Command::Free {
                ids: vec!["trash".into(), "temp".into(), "old-updates".into()],
                resume: Some("media".into())
            }]
        );
        let cmds = s.event(StoreEvent::Freed {
            freed: 1000,
            resumed: Some("media".into()),
        });
        assert!(cmds.contains(&Command::Install {
            app: "media".into(),
            path: "/usb/media.uap".into()
        }));
    }

    #[test]
    fn free_cancel_frees_nothing() {
        let mut s = store_with(&[("media", "1.0", &[], Some("m.uap"))]);
        s.act(StoreAction::Install("media".into()));
        let _ = s.event(StoreEvent::OperationDone {
            app: "media".into(),
            result: Err(StoreError::Backend(BackendError::NoSpace {
                required: None,
                available: None,
            })),
        });
        let cmds = s.act(StoreAction::FreeCancel);
        assert!(s.operation.is_none());
        assert!(s.free.entries.is_empty());
        assert!(cmds.iter().any(|c| matches!(c, Command::CancelOp { .. })));
    }

    #[test]
    fn cancel_stops_a_running_operation() {
        let mut s = store_with(&[("a", "1.0", &[], Some("a.uap"))]);
        s.act(StoreAction::Install("a".into()));
        let cmds = s.act(StoreAction::Cancel);
        assert_eq!(cmds, vec![Command::CancelOp { kind: OpKind::Install }]);
        assert!(s.operation.is_none());
    }

    #[test]
    fn retry_logs_and_offline_paths() {
        let mut s = store_with(&[("a", "1.0", &[], None)]);
        assert_eq!(s.act(StoreAction::Retry), vec![Command::ReloadCatalog]);
        s.last_error = Some(StoreError::from(SourceError::Damaged));
        let cmds = s.act(StoreAction::Log);
        assert_eq!(cmds.len(), 1);
        if let Command::SaveLog { report } = &cmds[0] {
            assert!(report.contains("Damaged"));
        } else {
            panic!("expected SaveLog");
        }
        let cmds = s.act(StoreAction::Offline);
        assert_eq!(s.area, Area::OfflineSources);
        assert_eq!(cmds, vec![Command::DiscoverSources]);
        s.act(StoreAction::UseSource("/media/USB2".into()));
    }

    #[test]
    fn offline_and_error_surfaces_still_show_installed() {
        let mut s = Store::new();
        let cmds = s.event(StoreEvent::CatalogLoaded(Err(StoreError::from(
            SourceError::Unreadable,
        ))));
        assert_eq!(s.surface, Surface::Offline);
        assert_eq!(cmds, vec![Command::ReloadInstalled]);
        let cmds = s.event(StoreEvent::CatalogLoaded(Err(StoreError::from(
            SourceError::Damaged,
        ))));
        assert!(matches!(s.surface, Surface::Error { .. }));
        assert_eq!(cmds, vec![Command::ReloadInstalled]);
    }

    #[test]
    fn catalogue_ready_then_installed_refresh() {
        let mut s = Store::new();
        let loaded = LoadedCatalog {
            catalog: Catalog {
                format_version: crate::catalog::FORMAT_VERSION,
                generated: None,
                apps: Vec::new(),
            },
            verified: true,
            signer: Some("k".into()),
            dir: Some("/usb".into()),
        };
        let cmds = s.event(StoreEvent::CatalogLoaded(Ok(loaded)));
        assert_eq!(s.surface, Surface::Ready);
        assert!(s.catalog.verified);
        assert_eq!(s.catalog.dir.as_deref(), Some("/usb"));
        assert_eq!(cmds, vec![Command::ReloadInstalled]);
        let cmds = s.event(StoreEvent::Installed(vec![InstalledApp {
            id: "a".into(),
            version: "1".into(),
            size: None,
            last_used_ms: None,
        }]));
        assert!(cmds.is_empty());
        assert!(s.is_installed("a"));
    }

    #[test]
    fn progress_and_done_update_the_operation() {
        let mut s = store_with(&[("a", "1.0", &[], Some("a.uap"))]);
        s.act(StoreAction::Install("a".into()));
        s.event(StoreEvent::Progress {
            app: "a".into(),
            percent: Some(0.5),
        });
        assert_eq!(
            s.operation.as_ref().unwrap().state("a"),
            Some(&OpState::Running(Some(0.5)))
        );
        let cmds = s.event(StoreEvent::OperationDone {
            app: "a".into(),
            result: Ok(()),
        });
        assert!(cmds.contains(&Command::ReloadInstalled));
        assert!(s.operation.is_none());
    }

    #[test]
    fn failed_operation_keeps_reason() {
        let mut s = store_with(&[("a", "1.0", &[], Some("a.uap"))]);
        s.act(StoreAction::Install("a".into()));
        s.event(StoreEvent::OperationDone {
            app: "a".into(),
            result: Err(StoreError::from(crate::error::BackendError::PackageRefused)),
        });
        assert_eq!(
            s.operation.as_ref().unwrap().state("a"),
            Some(&OpState::Failed)
        );
        assert_eq!(s.last_error.as_ref().unwrap().message_key(), "store.error.package-refused");
    }

    #[test]
    fn search_lives_and_clears() {
        let mut s = store_with(&[("office", "1.0", &[], None)]);
        s.act(StoreAction::Search("no-such-thing".into()));
        assert_eq!(s.area, Area::Search);
        assert!(visible_results(&s).is_empty());
        s.act(StoreAction::SearchClear);
        assert_eq!(s.area, Area::Discover);
        assert!(s.query.is_empty());
        assert!(visible_results(&s).is_empty());
    }

    #[test]
    fn remove_flow_confirms_then_removes() {
        let mut s = store_with(&[("a", "1.0", &[], None)]);
        let cmds = s.act(StoreAction::Remove("a".into()));
        assert_eq!(cmds, vec![Command::AskRemove("a".into())]);
        let cmds = s.act(StoreAction::RemoveConfirmed {
            app: "a".into(),
            keep_data: true,
        });
        assert_eq!(
            cmds,
            vec![Command::Uninstall {
                app: "a".into(),
                keep_data: true
            }]
        );
        let cmds = s.event(StoreEvent::Removed("a".into()));
        assert!(cmds.is_empty());
        assert!(!s.is_installed("a"));
        let _ = s.act(StoreAction::RemoveCancelled);
    }

    #[test]
    fn free_items_event_keeps_selection_and_sets_numbers() {
        let mut s = store_with(&[("a", "1.0", &[], None)]);
        s.free.entries = vec![FreeEntry {
            id: "temp".into(),
            size: None,
            selected: false,
        }];
        let cmds = s.event(StoreEvent::FreeItems(
            vec![
                FreeEntry {
                    id: "trash".into(),
                    size: Some(5),
                    selected: true,
                },
                FreeEntry {
                    id: "temp".into(),
                    size: Some(7),
                    selected: true,
                },
            ],
            Some(1),
            Some(2),
        ));
        assert!(cmds.is_empty());
        assert_eq!(s.free.required, Some(1));
        assert_eq!(s.free.available, Some(2));
        let temp = s.free.entries.iter().find(|f| f.id == "temp").unwrap();
        assert!(!temp.selected, "user selection must survive the refresh");
        assert_eq!(temp.size, Some(7));
        let trash = s.free.entries.iter().find(|f| f.id == "trash").unwrap();
        assert!(trash.selected);
        s.act(StoreAction::FreeItem("temp".into()));
        assert_eq!(s.free.selected_total(), 12);
    }

    #[test]
    fn category_page_from_chip() {
        let mut s = store_with(&[("a", "1.0", &[], None)]);
        s.category = None;
        let cmds = s.act(StoreAction::Category("office".into()));
        assert!(cmds.is_empty());
        assert_eq!(s.area, Area::Categories);
        assert_eq!(s.category.as_deref(), Some("office"));
    }

    #[test]
    fn install_file_takes_the_path_directly() {
        let mut s = Store::new();
        let cmds = s.act(StoreAction::InstallFile("/usb/tool.uap".into()));
        assert_eq!(
            cmds,
            vec![Command::Install {
                app: "/usb/tool.uap".into(),
                path: "/usb/tool.uap".into()
            }]
        );
        assert!(s.operation.is_some());
    }

    #[test]
    fn update_all_chains_the_updates() {
        let mut s = store_with(&[
            ("a", "2.0", &[], Some("a.uap")),
            ("b", "2.0", &[], Some("b.uap")),
        ]);
        s.installed = vec![
            InstalledApp { id: "a".into(), version: "1.0".into(), size: None, last_used_ms: None },
            InstalledApp { id: "b".into(), version: "1.0".into(), size: None, last_used_ms: None },
        ];
        let cmds = s.act(StoreAction::UpdateAll);
        assert_eq!(cmds.len(), 1);
        assert_eq!(s.operation.as_ref().unwrap().kind, OpKind::Update);
        // First done → the chain continues with the second.
        let cmds = s.event(StoreEvent::OperationDone { app: "a".into(), result: Ok(()) });
        assert!(matches!(cmds[0], Command::Install { .. }));
        let _ = s.event(StoreEvent::OperationDone { app: "b".into(), result: Ok(()) });
        assert!(s.update_queue.is_empty());
    }
}
