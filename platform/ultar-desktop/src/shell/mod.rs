//! What every click, key and service event does to the desktop (DESKTOP-SPEC §5, §13;
//! CLICKS.md). The controller changes the [`Desktop`] state and returns [`Effect`]s for
//! the world outside (compositor, services); it never draws.

mod bar;
mod grid;
mod keys;
mod menus;
mod render;
mod sheets;

pub use menus::{open_menu, MenuKind, MENU_KEYBOARD_SETTINGS};
pub use render::{build_bar, build_popup, status_icons, Popup};

use crate::model::time::{Clock, LocalTime};
use crate::model::{Desktop, Open};
use crate::view::Target;
use std::sync::Arc;
use ultar_bus::messages::settings::SettingValue;
use ultar_i18n::Translator;
use ultar_surface::proto::WindowAction;
use ultar_ui::event::ActionKind;

/// Something the shell asks of the world.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Start an app (with the activation token of the user's action).
    Launch {
        /// Application id.
        app: String,
        /// Arguments (`trash:///`, a Settings panel, `--new-window`).
        args: Vec<String>,
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
        }
    }

    /// Now in local time (the start of the epoch if the clock is off the calendar).
    pub fn local_time(&self) -> Option<LocalTime> {
        LocalTime::at(self.clock.now_ms(), self.clock.offset_s())
    }

    /// Opens `open`, or closes it when it is open already (a second click closes, §13).
    pub fn toggle(&mut self, open: Open) -> Outcome {
        let same = std::mem::discriminant(&self.d.open) == std::mem::discriminant(&open);
        self.d.open = if same { Open::None } else { open };
        if self.d.open == Open::Overview
            || matches!(self.d.open, Open::Apps(_))
            || self.d.open == Open::None
        {
            self.d.query.clear();
        }
        Outcome::surfaces()
    }

    /// Closes what is open (Esc, a click elsewhere, the compositor took the keyboard).
    pub fn close(&mut self) -> Outcome {
        if self.d.open == Open::None {
            return Outcome::none();
        }
        self.d.open = Open::None;
        self.d.query.clear();
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
            _ => sheets::on_popup(self, target, kind),
        }
    }

    /// Text typed into the search field of the open popup.
    pub fn on_query(&mut self, text: &str) -> Outcome {
        self.d.query = text.chars().take(crate::model::search::MAX_QUERY).collect();
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
}
