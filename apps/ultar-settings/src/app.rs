//! The app: state, services and the widget tree, glued together. Free of compositor and bus
//! types, so tests drive it with toolkit events and fake services.

use crate::error::AppError;
use crate::service::Ports;
use crate::state::State;
use crate::update::{self, Ctl, Outcome};
use crate::view::{self, View};
use ultar_bus::messages::settings::SettingValue;
use ultar_ui::event::{Action, Event, Response};
use ultar_ui::tokens::{SizeClass, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::widgets::Toast;

/// The setting the window's own theme follows.
const THEME_KEY: &str = "appearance.theme";

/// The Settings app.
pub struct App {
    /// The widget tree.
    pub ui: Ui,
    /// Handles of the tree.
    pub view: View,
    /// State.
    pub state: State,
    ports: Ports,
    toast_id: u32,
    toasts: u32,
    /// The width the page was built for.
    built_width: f32,
    /// The value of `appearance.theme` the window last followed.
    seen_theme: Option<String>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App").field("state", &self.state).finish()
    }
}

impl App {
    /// Loads the first page (area `area`) and builds the window.
    pub fn new(ui: Ui, ports: Ports, area: usize) -> Result<App, AppError> {
        let mut app = App {
            ui,
            view: View::default(),
            state: State::default(),
            ports,
            toast_id: 0,
            toasts: 0,
            built_width: 0.0,
            seen_theme: None,
        };
        app.built_width = app.ui.size().w;
        app.state.area = area.min(crate::areas::AREAS.len() - 1);
        app.state.compact = app.ui.size_class() == SizeClass::Compact;
        update::load_page(&mut app.state, &mut app.ports);
        view::build(&mut app.ui, &app.state, &mut app.view)?;
        app.seen_theme = app.stored_theme();
        Ok(app)
    }

    /// The stored value of the setting `appearance.theme`, if settingsd was read.
    fn stored_theme(&self) -> Option<String> {
        let items = self.state.panels.iter().flat_map(|p| p.items.iter());
        items
            .filter(|i| i.key == THEME_KEY)
            .find_map(|i| match &i.value {
                SettingValue::Text(t) => Some(t.clone()),
                _ => None,
            })
    }

    /// Takes the window's theme from the setting `appearance.theme` (`light`, `dark`; `auto`
    /// keeps what it has): the app's start calls it once, so the window opens in the stored
    /// theme.
    pub fn apply_stored_theme(&mut self) {
        self.seen_theme = self.stored_theme();
        match self.seen_theme.as_deref() {
            Some("light") => self.ui.set_theme(ThemeKind::Light),
            Some("dark") => self.ui.set_theme(ThemeKind::Dark),
            _ => {}
        }
    }

    /// A changed theme setting (here or elsewhere) takes effect in this window at once.
    fn follow_theme(&mut self) {
        if self.stored_theme() != self.seen_theme {
            self.apply_stored_theme();
        }
    }

    /// Feeds one input event; returns the toolkit's response.
    pub fn handle(&mut self, event: Event) -> Result<Response, AppError> {
        let response = self.ui.handle(event);
        self.actions(response.actions.clone())?;
        Ok(response)
    }

    /// Handles actions the toolkit emitted (also clock actions from `Ui::take_actions`).
    pub fn actions(&mut self, actions: Vec<Action>) -> Result<(), AppError> {
        for action in actions {
            let outcome = {
                let mut c = Ctl {
                    state: &mut self.state,
                    ports: &mut self.ports,
                    ui: &mut self.ui,
                    view: &self.view,
                };
                update::handle(&mut c, action.node, action.kind)
            };
            self.apply(outcome)?;
        }
        Ok(())
    }

    /// Opens an area as a click in the sidebar or a search result would (also areas the
    /// sidebar does not list on this size, e.g. the mobile network on a wide window).
    pub fn open_area(&mut self, area: usize) -> Result<(), AppError> {
        let outcome = update::open_area(&mut self.state, &mut self.ports, area);
        self.apply(outcome)
    }

    /// The surface changed size: a new size class rebuilds the window, a new width the page
    /// (pages choose their columns for the width they have: side by side when they fit, one
    /// below the other when not - a window made wider or narrower, or maximized, reflows).
    pub fn resized(&mut self) -> Result<(), AppError> {
        let compact = self.ui.size_class() == SizeClass::Compact;
        let width = self.ui.size().w;
        if compact != self.state.compact {
            self.state.compact = compact;
            self.state.compact_list = false;
            view::build(&mut self.ui, &self.state, &mut self.view)?;
            view::open_dialog(&mut self.ui, &self.state, &mut self.view)?;
        } else if width != self.built_width {
            self.apply(Outcome {
                rebuild: true,
                ..Outcome::default()
            })?;
        }
        self.built_width = width;
        Ok(())
    }

    /// Follows the services: settings changed elsewhere (a revert after 15 s, quick
    /// settings), new displays, update progress. Rebuilds only when what is shown changed.
    pub fn poll(&mut self) -> Result<(), AppError> {
        let before = (
            self.state.display.clone(),
            self.state.drivers.firmware.clone(),
            self.state.status.clone(),
        );
        if self.ports.settings.changed() {
            update::load_panels(&mut self.state, &mut self.ports);
        }
        update::load_page_light(&mut self.state, &mut self.ports);
        let after = (
            self.state.display.clone(),
            self.state.drivers.firmware.clone(),
            self.state.status.clone(),
        );
        if before != after && self.state.dialog.is_none() {
            self.apply(Outcome {
                rebuild: true,
                ..Outcome::default()
            })?;
        }
        self.follow_theme();
        Ok(())
    }

    fn apply(&mut self, outcome: Outcome) -> Result<(), AppError> {
        if outcome.rebuild_all {
            view::build(&mut self.ui, &self.state, &mut self.view)?;
        } else if outcome.rebuild {
            view::rebuild_content(&mut self.ui, &self.state, &mut self.view)?;
        }
        if outcome.nav {
            view::refresh_nav(&mut self.ui, &self.state, &mut self.view)?;
        }
        let reopen = (outcome.rebuild || outcome.rebuild_all) && self.state.dialog.is_some();
        if outcome.dialog || reopen {
            view::open_dialog(&mut self.ui, &self.state, &mut self.view)?;
        }
        if let Some((tone, text)) = outcome.toast {
            self.toast_id = self.toast_id.wrapping_add(1);
            self.toasts += 1;
            let label = crate::i18n::label(&text, self.ui.translator());
            self.ui.show_toast(Toast::new(self.toast_id, tone, label))?;
        }
        Ok(())
    }

    /// Number of messages shown so far (tests).
    pub fn toasts_shown(&self) -> u32 {
        self.toasts
    }
}
