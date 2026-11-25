//! login.access (board Login, DESKTOP-SPEC §5.3): the accessibility menu of the sign-in –
//! large text, high contrast, screen reader, on-screen keyboard. Each switch changes its
//! setting at once through the settings service's narrow right
//! (`settings.set_accessibility`, only `accessibility.*`); without the service the button is
//! disabled with its reason.

/// The switches of the menu in board order: (setting key, catalog key of the name).
pub const SWITCHES: [(&str, &str); 4] = [
    ("accessibility.large_text", "lock.access.large_text"),
    ("accessibility.high_contrast", "lock.access.high_contrast"),
    ("accessibility.screen_reader", "lock.access.screen_reader"),
    (
        "accessibility.on_screen_keyboard",
        "lock.access.on_screen_keyboard",
    ),
];

/// Which switches have something behind them: large text and high contrast change every
/// surface of the lock screen and the shell at once (`Look::with_access`); there is no screen
/// reader and no on-screen keyboard program in the system yet, so those two are disabled with
/// the reason instead of switching nothing (owner: "switching has no effect").
pub const AVAILABLE: [bool; 4] = [true, true, false, false];

/// The accessibility part of the lock screen's state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessState {
    /// The menu is open.
    pub open: bool,
    /// The values of [`SWITCHES`] as the settings service reported them; `None`: the
    /// service did not answer (the button is disabled with its reason).
    pub values: Option<[bool; 4]>,
}

impl AccessState {
    /// Keeps the values the service reported (keys it did not name stay off).
    pub fn reported(&mut self, values: &[(String, bool)]) {
        let mut out = [false; 4];
        for (slot, (key, _)) in out.iter_mut().zip(SWITCHES) {
            *slot = values.iter().any(|(k, on)| k == key && *on);
        }
        self.values = Some(out);
    }

    /// Switch `index` to `on`: the call to make, or `None` (no service, no such switch).
    pub fn toggle(&mut self, index: usize, on: bool) -> Option<(&'static str, bool)> {
        if !AVAILABLE.get(index).copied().unwrap_or(false) {
            return None;
        }
        let values = self.values.as_mut()?;
        let slot = values.get_mut(index)?;
        *slot = on;
        SWITCHES.get(index).map(|(key, _)| (*key, on))
    }

    /// Opens or closes the menu (only with the service).
    pub fn flip(&mut self) {
        self.open = !self.open && self.values.is_some();
    }
}
