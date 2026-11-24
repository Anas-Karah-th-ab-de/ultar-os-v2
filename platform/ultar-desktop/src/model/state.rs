//! Everything the bar and its sheets show, in one place.

use super::apps::{self, AppInfo};
use super::calendar::Month;
use super::notes::Notes;
use super::quick::{Presence, QuickValues};
use ultar_i18n::Lang;
use ultar_surface::proto::{OutputInfo, ShellWindow};
use ultar_ui::tokens::ThemeKind;

/// Which surface of the bar is open (one at a time; DESKTOP-SPEC §13 no. 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Open {
    /// Nothing.
    #[default]
    None,
    /// Quick settings, on a page.
    Quick(QuickPage),
    /// Calendar and notifications.
    Calendar,
    /// All apps (the app grid), showing one category or all (`None`).
    Apps(Option<super::apps::Category>),
    /// The overview (with the caret in its search field).
    Overview,
    /// The display mode panel (Super+P).
    Projection,
    /// The menu of the keyboard language button.
    KeyboardMenu,
    /// The power menu.
    PowerMenu,
    /// The menu of a bar tile (index into the tiles).
    TileMenu(usize),
}

/// Pages of the quick settings sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuickPage {
    /// The tiles.
    #[default]
    Tiles,
    /// The three power modes as a choice.
    PowerModes,
}

/// The keyboard layouts of the compositor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layouts {
    /// Names (`ar`, `en`).
    pub names: Vec<String>,
    /// Active index.
    pub active: u8,
}

/// The whole state of the desktop shell.
#[derive(Debug, Clone)]
pub struct Desktop {
    /// Language.
    pub lang: Lang,
    /// Colour theme.
    pub theme: ThemeKind,
    /// App catalog.
    pub catalog: Vec<AppInfo>,
    /// Pinned app ids.
    pub pinned: Vec<String>,
    /// Toplevel windows from the compositor.
    pub windows: Vec<ShellWindow>,
    /// Outputs from the compositor.
    pub outputs: Vec<OutputInfo>,
    /// Keyboard layouts.
    pub layouts: Layouts,
    /// Hardware and services.
    pub presence: Presence,
    /// Values of the quick settings.
    pub values: QuickValues,
    /// Notifications.
    pub notes: Notes,
    /// What is open.
    pub open: Open,
    /// Month shown by the calendar and the selected day index.
    pub calendar: Option<(Month, usize)>,
    /// Text of the overview search.
    pub query: String,
    /// Highlighted mode of the display mode panel (0–3).
    pub projection: usize,
    /// Settings id of the main display (`display.<id>.*`), once the settings service
    /// knows the displays.
    pub main_display: Option<String>,
}

impl Desktop {
    /// The start state: dark (standard, §11), the base apps pinned, nothing known yet.
    pub fn new(lang: Lang) -> Desktop {
        Desktop {
            lang,
            theme: ThemeKind::Dark,
            catalog: apps::base_apps(),
            pinned: apps::default_pinned(),
            windows: Vec::new(),
            outputs: Vec::new(),
            layouts: Layouts::default(),
            presence: Presence::default(),
            values: QuickValues {
                dark: true,
                ..QuickValues::default()
            },
            notes: Notes::default(),
            open: Open::None,
            calendar: None,
            query: String::new(),
            projection: 0,
            main_display: None,
        }
    }

    /// The tiles of the bar.
    pub fn bar_apps(&self) -> Vec<apps::BarApp> {
        apps::bar_apps(&self.pinned, &self.catalog, &self.windows)
    }

    /// The keyboard button is shown only with more than one input language.
    pub fn shows_keyboard(&self) -> bool {
        self.layouts.names.len() > 1
    }

    /// More than one display: the display mode panel can do something.
    pub fn several_displays(&self) -> bool {
        self.outputs.len() > 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_state_is_the_standard() {
        let d = Desktop::new(Lang::Ar);
        assert_eq!(d.theme, ThemeKind::Dark);
        assert_eq!(d.bar_apps().len(), 6);
        assert!(!d.shows_keyboard() && !d.several_displays());
        assert_eq!(d.open, Open::None);
    }
}
