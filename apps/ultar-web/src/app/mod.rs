//! The browser: one window of tabs over a [`WebEngine`]. It turns clicks and keys
//! ([`input`]), engine events ([`events`]) and panel answers ([`overlay`]) into engine
//! commands and model changes, and keeps the window ([`crate::chrome`]) in step.
//! Nothing here draws or talks to the compositor: [`crate::window`] does that, the tests
//! drive this type headless.

mod actions;
mod events;
pub mod handoff;
mod input;
mod overlay;
mod page;

pub use handoff::{FilesApp, Handoff, HandoffError};
pub use overlay::Overlay;

use crate::chrome::{Chrome, TabView, View};
use crate::engine::{EngineError, PermissionKind, RequestId, TabId, WebEngine};
use crate::model::downloads::Downloads;
use crate::model::history::History;
use crate::model::permissions::SitePermissions;
use crate::model::settings::Settings;
use crate::model::store::StoreError;
use crate::model::tabs::{Tab, Tabs};
use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;
use ultar_ui::ui::{Ui, UiError};

/// Most permission requests waiting for an answer.
pub const MAX_PENDING_PROMPTS: usize = 16;
/// History visits after which the history file is written.
const SAVE_EVERY: u32 = 20;

/// Where the browser keeps its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Profile folder (settings, history, site permissions).
    pub profile: PathBuf,
    /// Downloads folder.
    pub downloads: PathBuf,
}

impl Paths {
    fn settings(&self) -> PathBuf {
        self.profile.join("settings.tsv")
    }
    fn history(&self) -> PathBuf {
        self.profile.join("history.tsv")
    }
    fn permissions(&self) -> PathBuf {
        self.profile.join("permissions.tsv")
    }
}

/// Why the browser stopped or refused.
#[derive(Debug)]
pub enum AppError {
    /// The engine failed.
    Engine(EngineError),
    /// The window tree refused an operation.
    Ui(UiError),
    /// A state file could not be read or written.
    Store(StoreError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Engine(e) => write!(f, "{e}"),
            AppError::Ui(e) => write!(f, "{e}"),
            AppError::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<EngineError> for AppError {
    fn from(e: EngineError) -> AppError {
        AppError::Engine(e)
    }
}

impl From<UiError> for AppError {
    fn from(e: UiError) -> AppError {
        AppError::Ui(e)
    }
}

impl From<StoreError> for AppError {
    fn from(e: StoreError) -> AppError {
        AppError::Store(e)
    }
}

/// A permission request waiting for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The tab that asked.
    pub tab: TabId,
    /// The engine's request.
    pub request: RequestId,
    /// The site.
    pub host: String,
    /// What it asks for.
    pub kind: PermissionKind,
}

/// The browser's model.
#[derive(Debug, Default)]
pub struct State {
    /// Tabs.
    pub tabs: Tabs,
    /// Settings.
    pub settings: Settings,
    /// History (never of private tabs).
    pub history: History,
    /// Remembered site permissions.
    pub permissions: SitePermissions,
    /// Answers given in private tabs (forgotten with the window).
    pub private_permissions: SitePermissions,
    /// Downloads of this session.
    pub downloads: Downloads,
    /// Requests waiting for the prompt.
    pub prompts: VecDeque<Pending>,
    unsaved_visits: u32,
}

/// One browser window.
pub struct Browser<E: WebEngine> {
    engine: E,
    ui: Ui,
    chrome: Chrome,
    state: State,
    overlay: Overlay,
    paths: Option<Paths>,
    handoff: Box<dyn Handoff>,
    page_size: Option<(TabId, u32, u32)>,
    toast_id: u32,
    closed: bool,
}

impl<E: WebEngine> fmt::Debug for Browser<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Browser")
            .field("tabs", &self.state.tabs.len())
            .field("overlay", &self.overlay)
            .finish()
    }
}

impl<E: WebEngine> Browser<E> {
    /// A window with one tab on the start page (or at `url`). With `paths` the settings,
    /// history and site permissions are read from and written to the profile.
    pub fn new(
        engine: E,
        mut ui: Ui,
        paths: Option<Paths>,
        handoff: Box<dyn Handoff>,
    ) -> Result<Browser<E>, AppError> {
        let mut state = State::default();
        if let Some(p) = &paths {
            state.settings = Settings::load(&p.settings())?;
            state.history = History::load(&p.history())?;
            state.permissions = SitePermissions::load(&p.permissions())?;
        }
        let empty = View {
            tabs: Vec::new(),
            active: 0,
            can_back: false,
            can_forward: false,
            loading: false,
            address: String::new(),
            protection: true,
            start_page: true,
            page: None,
        };
        let chrome = Chrome::build(&mut ui, &empty)?;
        let mut browser = Browser {
            engine,
            ui,
            chrome,
            state,
            overlay: Overlay::None,
            paths,
            handoff,
            page_size: None,
            toast_id: 0,
            closed: false,
        };
        browser.new_tab(false)?;
        Ok(browser)
    }

    /// The window tree.
    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    /// The window tree, changeable (size, theme, language from the window).
    pub fn ui_mut(&mut self) -> &mut Ui {
        &mut self.ui
    }

    /// The window's nodes.
    pub fn chrome(&self) -> &Chrome {
        &self.chrome
    }

    /// The model.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// The engine.
    pub fn engine(&self) -> &E {
        &self.engine
    }

    /// The engine, changeable.
    pub fn engine_mut(&mut self) -> &mut E {
        &mut self.engine
    }

    /// What is open above the window.
    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    /// `true` once the last tab was closed (the window closes).
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// The view of the window for the current state.
    pub fn view(&self) -> View {
        let tabs = self.state.tabs.list();
        let active = self.state.tabs.active();
        let host = active.and_then(|t| crate::url::host_of(&t.url));
        View {
            tabs: tabs
                .iter()
                .map(|t| TabView {
                    title: t.label(),
                    private: t.private,
                })
                .collect(),
            active: self.state.tabs.active_index(),
            can_back: active.is_some_and(|t| t.can_back),
            can_forward: active.is_some_and(|t| t.can_forward),
            loading: active.is_some_and(|t| t.loading),
            address: active
                .map(|t| crate::url::readable(&t.url))
                .unwrap_or_default(),
            protection: host.map_or(self.state.settings.protection, |h| {
                self.state.settings.protects(&h)
            }),
            start_page: active.is_none_or(Tab::on_start_page),
            page: active.map(|t| t.engine),
        }
    }

    /// Brings the window in step with the model (rebuilding it when its structure changed)
    /// and the shown page's size in step with the window.
    pub fn refresh(&mut self) -> Result<(), AppError> {
        let view = self.view();
        if self.chrome.update(&mut self.ui, &view)? {
            self.overlay.forget();
            self.page_size = None;
            self.show_page_frame()?;
        }
        self.resize_page()?;
        self.show_next_prompt()?;
        Ok(())
    }

    /// Opens a tab on the start page after the shown one.
    pub fn new_tab(&mut self, private: bool) -> Result<(), AppError> {
        let id = self.engine.create_tab(private)?;
        if let Some(old) = self.state.tabs.active() {
            self.engine.set_visible(old.engine, false)?;
        }
        self.state.tabs.open(Tab::new(id, private));
        self.engine.set_visible(id, true)?;
        self.refresh()?;
        if let Some(field) = self.chrome.start_search {
            let _ = self.ui.focus(field);
        }
        Ok(())
    }

    /// Shows tab `index`.
    pub fn select_tab(&mut self, index: usize) -> Result<(), AppError> {
        let old = self.state.tabs.active().map(|t| t.engine);
        if !self.state.tabs.select(index) {
            return Ok(());
        }
        let new = self.state.tabs.active().map(|t| t.engine);
        if old != new {
            if let Some(old) = old {
                self.engine.set_visible(old, false)?;
            }
            if let Some(new) = new {
                self.engine.set_visible(new, true)?;
            }
        }
        self.refresh()
    }

    /// Closes tab `index`; closing the last tab closes the window.
    pub fn close_tab(&mut self, index: usize) -> Result<(), AppError> {
        let Some(tab) = self.state.tabs.close(index) else {
            return Ok(());
        };
        self.engine.close_tab(tab.engine)?;
        self.state.prompts.retain(|p| p.tab != tab.engine);
        if self.state.tabs.is_empty() {
            self.closed = true;
            self.save()?;
            return Ok(());
        }
        if let Some(t) = self.state.tabs.active() {
            self.engine.set_visible(t.engine, true)?;
        }
        if !self.state.tabs.list().iter().any(|t| t.private) {
            self.state.private_permissions = SitePermissions::default();
        }
        self.refresh()
    }

    /// Writes settings, history and site permissions (nothing without a profile).
    pub fn save(&mut self) -> Result<(), AppError> {
        if let Some(p) = &self.paths {
            self.state.settings.save(&p.settings())?;
            self.state.history.save(&p.history())?;
            self.state.permissions.save(&p.permissions())?;
            self.state.unsaved_visits = 0;
        }
        Ok(())
    }

    fn note_visit(&mut self) -> Result<(), AppError> {
        self.state.unsaved_visits += 1;
        if self.state.unsaved_visits >= SAVE_EVERY {
            if let Some(p) = &self.paths {
                self.state.history.save(&p.history())?;
            }
            self.state.unsaved_visits = 0;
        }
        Ok(())
    }

    /// Shows a short message (toast) with a catalog key.
    pub fn notify(&mut self, key: &str) {
        self.toast_id = self.toast_id.wrapping_add(1);
        let toast = ultar_ui::widgets::Toast::new(
            self.toast_id,
            ultar_ui::widgets::Tone::Info,
            ultar_ui::widgets::Label::key(key),
        );
        let _ = self.ui.show_toast(toast);
    }
}
