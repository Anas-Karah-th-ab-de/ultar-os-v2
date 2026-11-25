//! What the phone shows, without any drawing: the last `call.state` of the call service, the
//! dial pad input, the history filter, the open sheets and dialogs. The view
//! ([`crate::view`]) draws a [`Model`]; the app ([`crate::app`]) changes it.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use ultar_bus::messages::call::{
    is_emergency, CallInfo, CallState, Cellular, End, Failure, LogEntry, Phase, Ultar, Way,
    MAX_NUMBER_LEN, ULTAR_PREFIX,
};

/// Digits of an Ultar number without the prefix: `XXXX-XXXX`.
const ULTAR_DIGITS: usize = 8;
/// Position of the dash inside an Ultar number (after this many digits).
const ULTAR_GROUP: usize = 4;

/// Filter of the call history.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Filter {
    /// Every call.
    #[default]
    All,
    /// Missed calls only.
    Missed,
}

/// The surface the phone shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    /// Dial pad and call history (board PhoneDial).
    Dial,
    /// A call rings (board PhoneIncoming).
    Incoming(u32),
    /// A call is going on (board PhoneInCall).
    InCall(u32),
}

/// A dialog the phone has open.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Dialog {
    /// Ready messages after "decline with message".
    Text {
        /// Way of the declined call.
        way: Way,
        /// Number of the caller.
        number: String,
        /// Name or number for the title.
        name: String,
    },
    /// Confirmation before the screen is shared in call `id`.
    Share(u32),
}

/// Compact page (phone size): the dial pad or the history.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    /// Dial pad.
    #[default]
    Dial,
    /// History.
    Log,
}

/// Everything the phone shows.
#[derive(Clone, Debug, Default)]
pub struct Model {
    /// Last state of the call service.
    pub state: CallState,
    /// Local clock (ms) when `state` arrived.
    pub received_ms: u64,
    /// Local clock now (ms).
    pub now_ms: u64,
    /// Chosen way.
    pub way: Option<Way>,
    /// Typed number (without `UOS-` on the Ultar way).
    pub digits: String,
    /// History filter.
    pub filter: Filter,
    /// History, newest first.
    pub log: Vec<LogEntry>,
    /// In-call dial pad open.
    pub keypad: bool,
    /// Tones sent while the dial pad was open.
    pub tones: String,
    /// Open dialog.
    pub dialog: Option<Dialog>,
    /// Identifier of the ended call whose notice the user put away.
    pub dismissed: Option<u32>,
    /// Compact page.
    pub page: Page,
    /// Wall clock now, seconds since the Unix epoch (history times).
    pub wall_s: i64,
    /// Offset of the local time zone from UTC in seconds.
    pub utc_offset_s: i32,
    /// Which pictures of a video call arrived (cleared when no call is active).
    pub pictures: Pictures,
}

/// Which pictures of a video call arrived.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pictures {
    /// The other side's.
    pub other: bool,
    /// The own camera's.
    pub own: bool,
}

/// Maps Arabic-Indic and Eastern Arabic-Indic digits to 0–9 (DESKTOP-SPEC §3: digits are 0–9).
pub fn ascii_digit(c: char) -> Option<char> {
    match c {
        '0'..='9' | '*' | '#' | '+' => Some(c),
        '\u{0660}'..='\u{0669}' => char::from_digit(c as u32 - 0x0660, 10),
        '\u{06F0}'..='\u{06F9}' => char::from_digit(c as u32 - 0x06F0, 10),
        _ => None,
    }
}

/// Duration as `mm:ss` (or `h:mm:ss`).
pub fn format_duration(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

impl Model {
    /// The way the dial pad uses: the chosen one, else Ultar when signed in, else SIM 1.
    pub fn current_way(&self) -> Way {
        if let Some(way) = self.way {
            return way;
        }
        let lines = &self.state.lines;
        match (lines.ultar, lines.sims.first()) {
            (Ultar::SignedOut, Some(slot)) => Way::Cellular(*slot),
            _ => Way::Ultar,
        }
    }

    /// The SIM slot offered as the mobile-network way.
    pub fn cellular_way(&self) -> Way {
        Way::Cellular(self.state.lines.sims.first().copied().unwrap_or(1))
    }

    /// Takes a new state of the call service.
    pub fn set_state(&mut self, state: CallState, now_ms: u64) {
        let started = state.calls.iter().any(|c| c.phase.is_live());
        if started {
            self.dismissed = None;
        }
        if !state.calls.iter().any(|c| c.phase == Phase::Active) {
            self.keypad = false;
            self.tones.clear();
            self.pictures = Pictures::default();
        }
        if let Some(Dialog::Share(id)) = self.dialog {
            if !state.calls.iter().any(|c| c.id == id) {
                self.dialog = None;
            }
        }
        self.state = state;
        self.received_ms = now_ms;
        self.now_ms = now_ms;
    }

    /// The surface to show.
    pub fn screen(&self) -> Screen {
        let calls = &self.state.calls;
        if let Some(c) = calls.iter().find(|c| c.phase == Phase::Ringing) {
            return Screen::Incoming(c.id);
        }
        let order = [Phase::Active, Phase::Alerting, Phase::Dialing, Phase::Held];
        for phase in order {
            if let Some(c) = calls.iter().find(|c| c.phase == phase) {
                return Screen::InCall(c.id);
            }
        }
        Screen::Dial
    }

    /// The live call `id`.
    pub fn call(&self, id: u32) -> Option<&CallInfo> {
        self.state.calls.iter().find(|c| c.id == id)
    }

    /// The other live call next to `id` (held or waiting).
    pub fn other_call(&self, id: u32) -> Option<&CallInfo> {
        self.state.calls.iter().find(|c| c.id != id)
    }

    /// The full number to dial on the current way.
    pub fn number(&self) -> String {
        match self.current_way() {
            Way::Ultar => format!("{ULTAR_PREFIX}{}", self.digits),
            Way::Cellular(_) => self.digits.clone(),
        }
    }

    fn format_ultar(raw: &str) -> String {
        let digits: String = raw
            .chars()
            .filter(char::is_ascii_digit)
            .take(ULTAR_DIGITS)
            .collect();
        if digits.len() > ULTAR_GROUP {
            format!("{}-{}", &digits[..ULTAR_GROUP], &digits[ULTAR_GROUP..])
        } else {
            digits
        }
    }

    /// Replaces the typed number (text field input): keeps what a number can hold.
    pub fn set_typed(&mut self, text: &str) {
        let raw: String = text
            .trim_start_matches(ULTAR_PREFIX)
            .chars()
            .filter_map(ascii_digit)
            .collect();
        self.digits = match self.current_way() {
            Way::Ultar => Self::format_ultar(&raw),
            Way::Cellular(_) => raw.chars().take(MAX_NUMBER_LEN).collect(),
        };
    }

    /// Appends one key of the dial pad; false when the number is full or the key not allowed.
    pub fn push_key(&mut self, key: char) -> bool {
        let before = self.digits.clone();
        let mut next = before.clone();
        next.push(key);
        self.set_typed(&next);
        self.digits != before
    }

    /// Deletes the last digit (and a dash left in front of it).
    pub fn backspace(&mut self) -> bool {
        let had = !self.digits.is_empty();
        self.digits.pop();
        if self.digits.ends_with('-') {
            self.digits.pop();
        }
        had
    }

    /// Chooses the way; the typed number is re-formatted for it.
    pub fn choose_way(&mut self, way: Way) {
        self.way = Some(way);
        let typed = self.digits.clone();
        self.set_typed(&typed);
    }

    /// Why `way` cannot call now (`None` = it can).
    pub fn way_problem(&self, way: Way) -> Option<Failure> {
        let lines = &self.state.lines;
        match way {
            Way::Ultar => match lines.ultar {
                Ultar::Ready => None,
                Ultar::SignedOut => Some(Failure::SignedOut),
                Ultar::Offline => Some(Failure::Offline),
            },
            Way::Cellular(slot) => match lines.cellular {
                Cellular::Ready if lines.sims.contains(&slot) => None,
                Cellular::Ready | Cellular::NoSim | Cellular::NoModem => Some(Failure::NoSim),
                Cellular::Airplane => Some(Failure::Airplane),
                Cellular::NoNetwork => Some(Failure::NoNetwork),
                Cellular::EmergencyOnly => Some(Failure::EmergencyOnly),
            },
        }
    }

    /// The mobile-network way can be chosen at all (a SIM is present or emergency calls work).
    pub fn cellular_selectable(&self) -> bool {
        self.state.lines.cellular != Cellular::NoModem
    }

    /// The call button works for the typed number on the current way.
    pub fn can_call(&self) -> bool {
        let way = self.current_way();
        let number = self.number();
        if self.digits.is_empty() || self.screen() != Screen::Dial {
            return false;
        }
        match self.way_problem(way) {
            None => true,
            Some(_) => {
                matches!(way, Way::Cellular(_))
                    && self.cellular_selectable()
                    && is_emergency(&number)
            }
        }
    }

    /// Video calls work now (Ultar way, signed in, number typed).
    pub fn can_video(&self) -> bool {
        self.current_way() == Way::Ultar && self.can_call()
    }

    /// History entries under the filter.
    pub fn entries(&self) -> Vec<&LogEntry> {
        self.log
            .iter()
            .filter(|e| self.filter == Filter::All || e.missed)
            .collect()
    }

    /// Contact suggestion for the typed number: the newest named history entry of this way
    /// whose number contains the typed digits.
    pub fn suggestion(&self) -> Option<&LogEntry> {
        if self.digits.is_empty() {
            return None;
        }
        let typed = self.number();
        let way = self.current_way();
        self.log.iter().find(|e| {
            e.name.is_some()
                && (e.way == way || matches!((e.way, way), (Way::Cellular(_), Way::Cellular(_))))
                && e.number.contains(&typed)
        })
    }

    /// Seconds the call has been active.
    pub fn duration_s(&self, call: &CallInfo) -> u64 {
        let Some(since) = call.active_since_ms else {
            return 0;
        };
        let at_state = self.state.clock_ms.saturating_sub(since);
        let local = self.now_ms.saturating_sub(self.received_ms);
        (at_state + local) / 1000
    }

    /// The ended call whose notice the dial pad shows.
    pub fn ended(&self) -> Option<&CallInfo> {
        let ended = self.state.ended.as_ref()?;
        if self.dismissed == Some(ended.id) || !self.state.calls.is_empty() {
            return None;
        }
        match ended.phase {
            Phase::Ended(End::Blocked) => None,
            _ => Some(ended),
        }
    }

    /// Name or number of a call.
    pub fn who(call: &CallInfo) -> &str {
        call.name.as_deref().unwrap_or(&call.number)
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
