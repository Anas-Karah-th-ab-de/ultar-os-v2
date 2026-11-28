//! Everything the bar and its sheets show, in one place.

use super::apps::{self, AppInfo};
use super::calendar::Month;
use super::notes::Notes;
use super::quick::{Presence, QuickValues};
use ultar_i18n::Lang;
use ultar_surface::proto::{OutputInfo, ShellWindow};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::widgets::Thumbnail;

/// Where the desktop's bar is (DESKTOP-SPEC §11: one bar at the bottom is the standard; a
/// top bar with a dock is the variant of board DesktopTop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BarPlace {
    /// One bar at the bottom.
    #[default]
    Bottom,
    /// A top bar and a dock.
    Top,
}

impl BarPlace {
    /// The word of the option `--bar` (`bottom`, `top`).
    pub fn parse(word: &str) -> Option<BarPlace> {
        match word {
            "bottom" => Some(BarPlace::Bottom),
            "top" => Some(BarPlace::Top),
            _ => None,
        }
    }

    /// The value of the setting `bar.layout` (settings schema, panel "Desktop bar":
    /// `bottom_bar` = the standard, `top_bar_dock` = top bar and dock).
    pub fn from_setting(word: &str) -> Option<BarPlace> {
        match word {
            "bottom_bar" => Some(BarPlace::Bottom),
            "top_bar_dock" => Some(BarPlace::Top),
            _ => None,
        }
    }
}

/// The shell's shape: where the bar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shape {
    /// Bar at the bottom or at the top.
    pub bar: BarPlace,
}

impl Shape {
    /// The desktop with the top bar and the dock.
    pub const TOP: Shape = Shape { bar: BarPlace::Top };

    /// The desktop with the top bar and the dock.
    pub fn is_top(self) -> bool {
        self.bar == BarPlace::Top
    }
}

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
    /// The helper's sheet (Super+H).
    Helper,
    /// The display mode panel (Super+P) with its highlighted mode (0–3).
    Projection(usize),
    /// The menu of the keyboard language button.
    KeyboardMenu,
    /// The power menu.
    PowerMenu,
    /// The audio outputs (qs.output).
    OutputMenu,
    /// The menu of a drive tile (index into `Services::drives`).
    DriveMenu(usize),
    /// The menu of a bar tile (index into the tiles).
    TileMenu(usize),
    /// The menu of the active app (top bar, CLICKS `shell.appmenu`).
    AppMenu,
}

/// Pages of the quick settings sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuickPage {
    /// The tiles.
    #[default]
    Tiles,
    /// The three power modes as a choice.
    PowerModes,
    /// The Bluetooth devices (qs.bluetooth.list).
    Bluetooth,
    /// The Wi-Fi networks (qs.wifi.list).
    Wifi,
    /// The password of a Wi-Fi network (the join dialog, `Services::joining`).
    WifiJoin,
    /// The VPN connections (qs.vpn.list).
    Vpn,
}

/// The keyboard layouts of the compositor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layouts {
    /// Names (`ar`, `en`).
    pub names: Vec<String>,
    /// Active index.
    pub active: u8,
}

/// What the overview shows besides the windows (board Overview).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OverviewState {
    /// Text of the search.
    pub query: String,
    /// What the search looks through (Settings > Search).
    pub sources: super::search::Sources,
    /// The Store's cached, signature-checked catalogue (`search.source.store`); `None` until
    /// a verified cache is read. Apps from it that are not installed are offered.
    pub store: Option<ultar_catalog::Catalog>,
    /// The remembered searches and opened results (`search.history`).
    pub history: super::history::History,
    /// The conversation with the helper (asked from the search or in its own sheet).
    pub helper: super::helper::Helper,
    /// Shown workspace and how many there are (`Event::Workspaces`); `None` until the
    /// compositor said.
    pub workspaces: Option<(u8, u8)>,
    /// Copies of window contents by window id (at most [`MAX_THUMBS`], newest kept).
    pub thumbs: Vec<(u32, std::rc::Rc<Thumbnail>)>,
}

/// Most window thumbnails kept (the overview shows at most this many windows).
pub const MAX_THUMBS: usize = 12;

impl OverviewState {
    /// Keeps the copy of window `id` (replacing an older one, dropping the oldest beyond
    /// the limit).
    pub fn keep_thumb(&mut self, id: u32, thumb: std::rc::Rc<Thumbnail>) {
        self.thumbs.retain(|(w, _)| *w != id);
        self.thumbs.push((id, thumb));
        if self.thumbs.len() > MAX_THUMBS {
            self.thumbs.remove(0);
        }
    }

    /// The copy of window `id`, if one came.
    pub fn thumb(&self, id: u32) -> Option<std::rc::Rc<Thumbnail>> {
        self.thumbs
            .iter()
            .find(|(w, _)| *w == id)
            .map(|(_, t)| t.clone())
    }

    /// Forgets the copies of windows that went.
    pub fn retain_windows(&mut self, ids: &[u32]) {
        self.thumbs.retain(|(w, _)| ids.contains(w));
    }
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
    /// The overview: its search text, the workspaces, the window thumbnails.
    pub overview: OverviewState,
    /// What the system services report (and the main display's settings id).
    pub svc: super::services::Services,
    /// Accent, text size, clock, time zone and bar icons as set in Settings.
    pub prefs: super::prefs::Prefs,
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
            overview: OverviewState::default(),
            svc: super::services::Services::default(),
            prefs: super::prefs::Prefs::default(),
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

    #[test]
    fn the_bar_place_follows_the_option_and_the_setting() {
        assert_eq!(BarPlace::parse("top"), Some(BarPlace::Top));
        assert_eq!(BarPlace::parse("bottom"), Some(BarPlace::Bottom));
        assert_eq!(BarPlace::parse("left"), None);
        assert_eq!(BarPlace::from_setting("top_bar_dock"), Some(BarPlace::Top));
        assert_eq!(BarPlace::from_setting("bottom_bar"), Some(BarPlace::Bottom));
        assert_eq!(BarPlace::from_setting("top"), None);
        assert!(Shape::TOP.is_top() && !Shape::default().is_top());
    }
}
