//! Clicks and answers → state + commands (APP-RULES §2 `update.rs`): pure
//! logic, testable without a screen. The view calls [`Store::act`] for every
//! user action; the shell feeds [`StoreEvent`] answers back. Nothing here
//! touches I/O.

use crate::action::{Command, StoreAction, StoreEvent};
use crate::error::{BackendError, StoreError};
use crate::state::{
    default_free_entries, Area, FreeEntry, FreeSpace, OpKind, OpState, Operation, Store, Surface,
};

impl Store {
    /// Handles one user action; returns the commands the shell must run.
    pub fn act(&mut self, action: StoreAction) -> Vec<Command> {
        match action {
            StoreAction::Area(area) => self.act_area(area),
            StoreAction::Category(cat) => {
                self.query.clear();
                self.category = Some(cat);
                self.area = Area::Categories;
                self.open_app = None;
                Vec::new()
            }
            StoreAction::Search(q) => self.act_search(q),
            StoreAction::SearchClear => {
                self.query.clear();
                self.area = Area::Discover;
                Vec::new()
            }
            StoreAction::More => Vec::new(),
            StoreAction::Open(app) => self.open_app_page(app),
            StoreAction::Back => {
                // Returns to the list the page was opened from; the list is
                // still there with the same query (CLICKS `store.back`).
                self.area = self.back_to.take().unwrap_or(Area::Discover);
                self.open_app = None;
                Vec::new()
            }
            action => self.act_operations(action),
        }
    }

    /// `store.area`: the areas of the nav, the tabs and the more menu.
    fn act_area(&mut self, area: Area) -> Vec<Command> {
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

    /// `store.search`: live search; an empty query shows Discover again.
    fn act_search(&mut self, query: String) -> Vec<Command> {
        self.query = query;
        if self.query.is_empty() {
            if self.area == Area::Search {
                self.area = Area::Discover;
            }
        } else {
            self.area = Area::Search;
        }
        Vec::new()
    }

    /// `store.open`: the app page, remembering where it came from.
    fn open_app_page(&mut self, app: String) -> Vec<Command> {
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

    /// Install/update/remove and the surfaces around them.
    fn act_operations(&mut self, action: StoreAction) -> Vec<Command> {
        match action {
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
            StoreAction::Remove(app) => {
                // Dangerous action: the confirmation dialog comes first
                // (keep the data or delete it), nothing is removed yet.
                self.pending_remove = Some(app.clone());
                vec![Command::AskRemove(app)]
            }
            StoreAction::RemoveConfirmed { app, keep_data } => {
                self.pending_remove = None;
                self.operation = Some(Operation::started(OpKind::Remove, &app));
                vec![Command::Uninstall { app, keep_data }]
            }
            StoreAction::RemoveCancelled => {
                self.pending_remove = None;
                Vec::new()
            }
            StoreAction::Cancel => match self.operation.take() {
                Some(op) => {
                    self.free.entries.clear();
                    self.update_queue.clear();
                    vec![Command::CancelOp { kind: op.kind }]
                }
                None => Vec::new(),
            },
            action => self.act_surfaces(action),
        }
    }

    /// The surface actions: retry, log, offline sources and freeing space.
    fn act_surfaces(&mut self, action: StoreAction) -> Vec<Command> {
        match action {
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
                let resume = self.operation.as_ref().and_then(|o| o.no_space_for.clone());
                vec![Command::Free { ids, resume }]
            }
            StoreAction::FreeCancel => {
                // Cancels the installation; nothing is freed (CLICKS).
                let kind = self
                    .operation
                    .as_ref()
                    .map(|o| o.kind)
                    .unwrap_or(OpKind::Install);
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
            // Handled one level up; unreachable here.
            _ => Vec::new(),
        }
    }

    /// Starts an install/update if the catalogue carries the package file and
    /// every requirement is installed.
    fn begin_op(&mut self, kind: OpKind, app: &str) -> Vec<Command> {
        let Some(entry) = self.entry(app) else {
            return Vec::new();
        };
        let missing = entry.requires.iter().any(|r| !self.is_installed(r));
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
                self.apply_free_items(items, required, available)
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

    /// Replaces the free list with the launcher's numbers, keeping the user's
    /// selection per item id (a new item starts selected, like the board).
    fn apply_free_items(
        &mut self,
        items: Vec<FreeEntry>,
        required: Option<u64>,
        available: Option<u64>,
    ) -> Vec<Command> {
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
                vec![Command::FreeItems { for_app: Some(app) }]
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

/// The hits for the current query (views and tests).
pub use crate::search::visible_results;
