//! What every click, key and service event does to the desktop (DESKTOP-SPEC §5, §13;
//! CLICKS.md). The controller changes the [`Desktop`] state and returns [`Effect`]s for
//! the world outside (compositor, services); it never draws.

mod bar;
mod desk;
mod grid;
mod handoff;
mod helper;
mod history;
mod keys;
mod media;
mod menus;
mod network;
mod render;
mod shapes;
mod sheets;

pub use desk::{item as desk_item, shortcut as shortcut_item};
pub use menus::{open_menu, MenuKind, MENU_KEYBOARD_SETTINGS};
pub use render::{build_bar, build_popup, status_icons, Popup};
pub use shapes::{bar_panel, build_second, second_panel};

use crate::model::time::{Clock, LocalTime};
use crate::model::{Desktop, Open, Shape};
use crate::view::Target;
use std::sync::Arc;
use ultar_bus::messages::settings::SettingValue;
use ultar_i18n::Translator;
use ultar_surface::proto::{ShellWindow, WindowAction};
use ultar_ui::event::ActionKind;

/// Something the shell asks of the world.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Start an app (with the activation token of the user's action).
    Launch {
        /// Application id.
        app: String,
        /// What to open in it (`launcher.start` version 2).
        open: Opening,
    },
    /// Act on a window.
    Window(u32, WindowAction),
    /// Change a setting.
    Setting(String, SettingValue),
    /// Switch the display mode (0 this device, 1 mirror, 2 extend, 3 second only).
    DisplayMode(usize),
    /// Lock the screen.
    Lock,
    /// Select a keyboard layout.
    Layout(u8),
    /// Show a workspace (one past the last adds one).
    Workspace(u8),
    /// Ask for copies of these windows' contents, at most this size in pixels.
    Thumbnails(Vec<u32>, (u16, u16)),
    /// Call a system service.
    Service(crate::model::services::Call),
    /// Keep the search history beside the shell (save it when `search.history` is on,
    /// delete it when off). The entries are already in the state.
    History,
    /// Keep the desktop's shortcuts beside the shell (they are already in the state).
    Shortcuts,
}

/// What an app start opens (`launcher.start` version 2, docs/integration/ultar-launcher.md):
/// a place of the app as one URI of a scheme its manifest declares, or another window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Opening {
    /// `trash:///`, `settings://network`, `store://app/<id>` …; `None`: start or raise.
    pub uri: Option<String>,
    /// Another window even when the app runs.
    pub new_window: bool,
}

impl Opening {
    /// Start the app or raise its window.
    pub fn start() -> Opening {
        Opening::default()
    }

    /// Open `uri` in the app.
    pub fn uri(uri: impl Into<String>) -> Opening {
        Opening {
            uri: Some(uri.into()),
            new_window: false,
        }
    }

    /// A page of Settings (`settings://<page>`).
    pub fn settings_page(page: &str) -> Opening {
        Opening::uri(format!("settings://{page}"))
    }

    /// The Store's page of an app (`store://app/<id>`), or its removal dialog.
    pub fn store_page(app: &str, remove: bool) -> Opening {
        let action = if remove { "?action=remove" } else { "" };
        Opening::uri(format!("store://app/{app}{action}"))
    }

    /// Another window of the app.
    pub fn new_window() -> Opening {
        Opening {
            uri: None,
            new_window: true,
        }
    }
}

/// What changed for the host.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    /// Effects to run.
    pub effects: Vec<Effect>,
    /// The bar must be built again.
    pub bar: bool,
    /// The open popup must be built again (or was opened / closed).
    pub popup: bool,
}

impl Outcome {
    /// Nothing to do.
    pub fn none() -> Outcome {
        Outcome::default()
    }

    /// The popup (and the bar's open marks) changed.
    pub fn surfaces() -> Outcome {
        Outcome {
            effects: Vec::new(),
            bar: true,
            popup: true,
        }
    }

    /// One effect, nothing to rebuild.
    pub fn effect(effect: Effect) -> Outcome {
        Outcome {
            effects: vec![effect],
            bar: false,
            popup: false,
        }
    }

    /// One effect and closing what is open.
    pub fn effect_and_close(effect: Effect) -> Outcome {
        Outcome {
            effects: vec![effect],
            bar: true,
            popup: true,
        }
    }
}

/// The desktop shell's logic.
pub struct Shell {
    /// The state.
    pub d: Desktop,
    /// Texts of the current language.
    pub tr: Translator,
    /// Wall clock.
    pub clock: Arc<dyn Clock>,
    /// Name shown for the signed-in user.
    pub user: String,
    /// Bar at the bottom or at the top.
    pub shape: Shape,
    /// The display mode chosen last (where Super+P starts).
    pub projection: usize,
    /// The drive ids last seen in `volumes.state` (`None` until the first report): a new id
    /// is a drive plugged in, which `apps.removable_media` reacts to.
    pub media_seen: Option<Vec<String>>,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell")
            .field("open", &self.d.open)
            .finish_non_exhaustive()
    }
}

impl Shell {
    /// A shell for a language.
    pub fn new(d: Desktop, clock: Arc<dyn Clock>, user: &str) -> Shell {
        let tr = crate::i18n::translator_or_keys(d.lang);
        Shell {
            d,
            tr,
            clock,
            user: user.to_owned(),
            shape: Shape::default(),
            projection: 0,
            media_seen: None,
        }
    }

    /// Now in local time (the start of the epoch if the clock is off the calendar).
    pub fn local_time(&self) -> Option<LocalTime> {
        LocalTime::at(self.clock.now_ms(), self.offset_s())
    }

    /// Offset of the local time: the time zone of the settings (`datetime.timezone.name`),
    /// the clock's own until the service said.
    pub fn offset_s(&self) -> i32 {
        self.d
            .prefs
            .offset_s(self.clock.now_ms(), self.clock.offset_s())
    }

    /// Switches every text to `lang` (`region.language` changed while the shell runs).
    pub fn set_lang(&mut self, lang: ultar_i18n::Lang) {
        self.d.lang = lang;
        self.tr = crate::i18n::translator_or_keys(lang);
    }

    /// Opens `open`, or closes it when it is open already (a second click closes, §13).
    pub fn toggle(&mut self, open: Open) -> Outcome {
        let same = std::mem::discriminant(&self.d.open) == std::mem::discriminant(&open);
        self.d.open = if same { Open::None } else { open };
        self.d.places.picking = false;
        if self.d.open == Open::Overview
            || matches!(self.d.open, Open::Apps(_))
            || self.d.open == Open::None
        {
            self.d.overview.query.clear();
        }
        let mut out = Outcome::surfaces();
        if self.d.open == Open::Overview {
            out.effects.extend(self.overview_thumbnails());
        }
        if self.d.open == Open::Calendar {
            out.effects.extend(self.day_request());
        }
        out
    }

    /// The appointments of the day the calendar sheet shows (`calendar.day`).
    pub fn day_request(&self) -> Option<Effect> {
        let today = self.local_time()?.date;
        let day = crate::view::calendar::shown_day(&self.d, today);
        let year = u16::try_from(day.year()).ok()?;
        Some(Effect::Service(crate::model::services::Call::Day(
            year,
            day.month(),
            day.day(),
        )))
    }

    /// The overview opened: fresh copies of the windows it shows.
    pub fn overview_thumbnails(&self) -> Option<Effect> {
        let ids: Vec<u32> = crate::view::overview::shown(&self.d)
            .iter()
            .map(|w| w.id)
            .collect();
        (!ids.is_empty()).then(|| crate::view::overview::thumbnails(&self.d, &ids))
    }

    /// Closes what is open (Esc, a click elsewhere, the compositor took the keyboard).
    pub fn close(&mut self) -> Outcome {
        if self.d.open == Open::None {
            return Outcome::none();
        }
        self.d.open = Open::None;
        self.d.overview.query.clear();
        self.d.places.picking = false;
        self.d.places.draft.clear();
        self.d.svc.wifi.joining = None;
        self.d.svc.mesh.targets.clear();
        Outcome::surfaces()
    }

    /// An action of the bar's tree.
    pub fn on_bar(&mut self, target: &Target, kind: &ActionKind) -> Outcome {
        bar::on_bar(self, target, kind)
    }

    /// An action of the open popup's tree.
    pub fn on_popup(&mut self, target: &Target, kind: &ActionKind) -> Outcome {
        match (target, kind) {
            (Target::AppItem(app), ActionKind::MenuChosen(id)) => {
                grid::app_menu(self, &app.clone(), *id)
            }
            (Target::Categories, ActionKind::Selected(i)) => grid::category(self, *i),
            (Target::RenameField, ActionKind::Submitted)
            | (Target::RenameSave, ActionKind::Activated) => desk::rename_done(self),
            (Target::RenameCancel, ActionKind::Activated) => self.close(),
            _ => sheets::on_popup(self, target, kind),
        }
    }

    /// An action of a tree that is neither the bar nor the popup (the dock): by what the
    /// node means.
    pub fn on_surface(&mut self, target: &Target, kind: &ActionKind) -> Outcome {
        match target {
            Target::Trash
            | Target::Overview
            | Target::AllApps
            | Target::Search
            | Target::App(_)
            | Target::Keyboard
            | Target::Status
            | Target::Clock
            | Target::AppMenu => self.on_bar(target, kind),
            _ => self.on_popup(target, kind),
        }
    }

    /// The compositor's window list changed.
    pub fn on_windows(&mut self, windows: Vec<ShellWindow>) -> Outcome {
        let mut out = Outcome::surfaces();
        let ids: Vec<u32> = windows.iter().map(|w| w.id).collect();
        self.d.overview.retain_windows(&ids);
        let fresh: Vec<u32> = windows
            .iter()
            .filter(|w| !self.d.windows.contains(w))
            .map(|w| w.id)
            .collect();
        self.d.windows = windows;
        if self.d.open == Open::Overview && !fresh.is_empty() {
            out.effects
                .push(crate::view::overview::thumbnails(&self.d, &fresh));
        }
        out
    }

    /// Text typed into the search field of the open popup.
    pub fn on_query(&mut self, text: &str) -> Outcome {
        if self.d.open == Open::Helper {
            return helper::typed(self, text);
        }
        self.d.overview.query = text.chars().take(crate::model::search::MAX_QUERY).collect();
        Outcome {
            effects: Vec::new(),
            bar: false,
            popup: true,
        }
    }

    /// A shell shortcut from the compositor.
    pub fn on_shell_key(&mut self, code: u16) -> Outcome {
        keys::on_shell_key(self, code)
    }

    /// A key the open popup did not use (Esc, Enter, Super+P inside the panel).
    pub fn on_popup_key(&mut self, key: &ultar_ui::event::Key) -> Outcome {
        keys::on_popup_key(self, key)
    }

    /// The secondary button or the menu key on a bar node.
    pub fn on_bar_menu(&mut self, target: &Target) -> Outcome {
        menus::open_for(self, target)
    }

    /// A command of the open menu.
    pub fn on_menu(&mut self, id: u32) -> Outcome {
        menus::chosen(self, id)
    }

    /// An action on the desktop background (a shortcut selected or opened).
    pub fn on_desk(&mut self, target: &Target, kind: &ActionKind) -> Outcome {
        desk::on_desk(self, target, kind)
    }

    /// The secondary button or the menu key on the desktop background at screen point
    /// `at`: on a shortcut (`target`) its menu, elsewhere the background's.
    pub fn on_desk_menu(&mut self, target: Option<&Target>, at: (i32, i32)) -> Outcome {
        desk::open_at(self, target, at)
    }

    /// Text typed into the rename dialog of a shortcut.
    pub fn on_rename_text(&mut self, text: &str) -> Outcome {
        desk::typed(self, text)
    }

    /// Drives with media appeared (`volumes.state`): `apps.removable_media` decides what
    /// happens – ask, open in Files, or nothing.
    pub fn media_inserted(&mut self) -> Outcome {
        media::inserted(self)
    }

    /// A notification's button was pressed: the shell's own notice opens its place (a
    /// plugged-in drive) in Files; a service notification's button goes back to the sender.
    pub fn note_action(&mut self, id: u64, index: u8) -> Outcome {
        let Some(action) = self.d.notes.action(id, index as usize) else {
            return Outcome::none();
        };
        if id >= crate::model::notes::LOCAL_IDS - 1024 {
            self.d.notes.dismiss(id);
            Outcome::effect_and_close(Effect::Launch {
                app: bar::FILES.to_owned(),
                open: Opening::uri(action),
            })
        } else {
            Outcome::effect(Effect::Service(crate::model::services::Call::NoteAction(
                id, action,
            )))
        }
    }
}
