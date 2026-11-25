//! State of the phone shell without drawing. Everything here is plain data with pure
//! functions, so that views and clicks are tested without a compositor.

pub mod apps;
pub mod quick;
pub mod time;

use apps::App;
use quick::{Hardware, Quick, Toggle};
use time::LocalTime;

/// What fills the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// The lock screen.
    Lock,
    /// The home screen.
    Home,
    /// The quick settings, pulled down over the home screen.
    Quick,
    /// The recently used apps.
    Recents,
    /// The search over apps and settings.
    Search,
}

/// Strength of a radio signal in bars 0–4; `None` = no connection.
pub type Bars = Option<u8>;

/// What the status bar and the device card show.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// Battery level in percent, if there is a battery.
    pub battery: Option<u8>,
    /// The battery is charging.
    pub charging: bool,
    /// Wi-Fi signal.
    pub wifi: Bars,
    /// Mobile signal (only with a modem).
    pub cell: Bars,
    /// Processor load in percent.
    pub cpu: u8,
    /// Memory in use in percent.
    pub ram: u8,
}

/// A notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Its number (to dismiss it).
    pub id: u64,
    /// Identifier of the app it came from.
    pub app: String,
    /// The title (as the app sent it).
    pub title: String,
    /// The text.
    pub body: String,
    /// `16:20`.
    pub time: String,
}

/// The whole phone shell.
#[derive(Debug, Clone, PartialEq)]
pub struct Mobile {
    /// What fills the screen.
    pub screen: Screen,
    /// The screen is locked (the lock screen comes back on every wake).
    pub locked: bool,
    /// The page of the app grid (from 0).
    pub page: usize,
    /// Installed apps in home screen order.
    pub apps: Vec<App>,
    /// Identifiers of the dock apps.
    pub dock: Vec<String>,
    /// Recently used apps, newest first.
    pub recents: Vec<String>,
    /// Now.
    pub now: Option<LocalTime>,
    /// Battery, radios, load.
    pub status: Status,
    /// What the phone has.
    pub hardware: Hardware,
    /// Quick settings.
    pub quick: Quick,
    /// Notifications, newest first.
    pub notes: Vec<Note>,
    /// The next appointment (`14:00 · Dentist`), if any today.
    pub next: Option<String>,
    /// Text in the search field.
    pub query: String,
    /// The power menu is open (over the quick settings).
    pub power_menu: bool,
}

impl Default for Mobile {
    fn default() -> Mobile {
        Mobile {
            screen: Screen::Lock,
            locked: true,
            page: 0,
            apps: apps::system_apps(),
            dock: apps::default_dock(),
            recents: Vec::new(),
            now: None,
            status: Status::default(),
            hardware: Hardware::PHONE,
            quick: Quick::default(),
            notes: Vec::new(),
            next: None,
            query: String::new(),
            power_menu: false,
        }
    }
}

impl Mobile {
    /// The app with identifier `id`.
    pub fn app(&self, id: &str) -> Option<&App> {
        self.apps.iter().find(|a| a.id == id)
    }

    /// The apps of the grid (the dock apps are not repeated there).
    pub fn grid_apps(&self) -> Vec<App> {
        self.apps
            .iter()
            .filter(|a| !self.dock.contains(&a.id))
            .cloned()
            .collect()
    }

    /// The dock apps that are installed, in dock order.
    pub fn dock_apps(&self) -> Vec<&App> {
        self.dock.iter().filter_map(|id| self.app(id)).collect()
    }

    /// Number of grid pages with `per_page` apps.
    pub fn page_count(&self, per_page: usize) -> usize {
        apps::pages(&self.grid_apps(), per_page).len()
    }

    /// Shows `page` (kept inside the pages there are).
    pub fn show_page(&mut self, page: usize, per_page: usize) {
        self.page = page.min(self.page_count(per_page).saturating_sub(1));
    }

    /// A swipe on the grid: `forward` = towards the next page.
    pub fn turn_page(&mut self, forward: bool, per_page: usize) {
        let page = if forward {
            self.page.saturating_add(1)
        } else {
            self.page.saturating_sub(1)
        };
        self.show_page(page, per_page);
    }

    /// Goes to `screen`; the lock screen cannot be left this way (only by [`Mobile::unlocked`]).
    pub fn go(&mut self, screen: Screen) {
        if self.locked && screen != Screen::Lock {
            return;
        }
        if screen != Screen::Search {
            self.query.clear();
        }
        self.power_menu = false;
        self.screen = screen;
    }

    /// The screen locks (power button, time-out).
    pub fn lock(&mut self) {
        self.locked = true;
        self.power_menu = false;
        self.screen = Screen::Lock;
        self.query.clear();
    }

    /// The person was recognised (password, PIN, fingerprint – checked by the platform).
    pub fn unlocked(&mut self) {
        self.locked = false;
        self.screen = Screen::Home;
    }

    /// Back: closes what lies over the home screen.
    pub fn back(&mut self) {
        match self.screen {
            Screen::Quick | Screen::Recents | Screen::Search => self.go(Screen::Home),
            Screen::Home | Screen::Lock => {}
        }
    }

    /// An app was started: it goes to the front of the recent apps.
    pub fn started(&mut self, id: &str) {
        self.recents.retain(|r| r != id);
        self.recents.insert(0, id.to_owned());
        self.recents.truncate(MAX_RECENTS);
        if !self.locked {
            self.screen = Screen::Home;
        }
        self.query.clear();
    }

    /// Removes one app from the recent apps.
    pub fn forget_recent(&mut self, id: &str) {
        self.recents.retain(|r| r != id);
    }

    /// Switches `t`, if the phone offers it.
    pub fn toggle(&mut self, t: Toggle) -> Option<bool> {
        if !self.hardware.offers(t) {
            return None;
        }
        let on = !self.quick.is_on(t);
        self.quick.set(t, on);
        Some(on)
    }

    /// Removes notification `id`.
    pub fn dismiss(&mut self, id: u64) {
        self.notes.retain(|n| n.id != id);
    }

    /// The apps that match the search text.
    pub fn hits(&self, shown: impl Fn(&App) -> String) -> Vec<&App> {
        apps::matching(&self.apps, &self.query, shown)
    }
}

/// Recent apps kept.
pub const MAX_RECENTS: usize = 12;

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Mobile {
        let mut m = Mobile::default();
        m.unlocked();
        m
    }

    #[test]
    fn the_lock_screen_is_only_left_by_unlocking() {
        let mut m = Mobile::default();
        m.go(Screen::Home);
        assert_eq!(m.screen, Screen::Lock);
        m.unlocked();
        assert_eq!(m.screen, Screen::Home);
        m.go(Screen::Quick);
        m.lock();
        assert_eq!((m.screen, m.locked), (Screen::Lock, true));
    }

    #[test]
    fn back_closes_what_lies_over_the_home_screen() {
        let mut m = open();
        for s in [Screen::Quick, Screen::Recents, Screen::Search] {
            m.go(s);
            m.back();
            assert_eq!(m.screen, Screen::Home);
        }
    }

    #[test]
    fn pages_stay_inside_and_the_dock_is_not_in_the_grid() {
        let mut m = open();
        let grid = m.grid_apps();
        assert!(grid.iter().all(|a| !m.dock.contains(&a.id)));
        assert_eq!(m.page_count(4), 2);
        m.turn_page(true, 4);
        m.turn_page(true, 4);
        assert_eq!(m.page, 1);
        m.turn_page(false, 4);
        m.turn_page(false, 4);
        assert_eq!(m.page, 0);
    }

    #[test]
    fn a_started_app_goes_to_the_front_of_the_recents() {
        let mut m = open();
        m.started("a");
        m.started("b");
        m.started("a");
        assert_eq!(m.recents, vec!["a".to_owned(), "b".to_owned()]);
        m.forget_recent("a");
        assert_eq!(m.recents, vec!["b".to_owned()]);
        for i in 0..20 {
            m.started(&i.to_string());
        }
        assert_eq!(m.recents.len(), MAX_RECENTS);
    }

    #[test]
    fn a_switch_the_phone_lacks_does_nothing() {
        let mut m = open();
        m.hardware.modem = false;
        assert_eq!(m.toggle(Toggle::MobileData), None);
        assert_eq!(m.toggle(Toggle::Dnd), Some(true));
    }
}
