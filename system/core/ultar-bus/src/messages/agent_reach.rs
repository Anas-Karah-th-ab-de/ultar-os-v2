//! What the helper reaches, and what it did: the two things the person can look at.
//!
//! The helper reaches every part of the system that has a service on the bus – always
//! with its own identity and the bus rules of that identity. The person switches whole
//! areas off, sees every tool an area holds, and reads what the helper did.

use super::agent::{Brain, Step, SERVICE};
use crate::message::{Call, Message, Text, Topic};
use serde::{Deserialize, Serialize};

/// A part of the system the helper can reach.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum Area {
    /// Explaining the system; touches nothing.
    #[default]
    Help,
    /// Apps: list, open, close.
    Apps,
    /// Sound and media buttons.
    Sound,
    /// Wi-Fi, Bluetooth, airplane mode.
    Network,
    /// Screenshot and lock screen.
    Screen,
    /// Every setting of the system.
    Settings,
    /// Notifications.
    Notifications,
    /// Drives and how full they are.
    Storage,
    /// The person's other devices (mesh).
    Devices,
    /// Sleep, restart, power off, sign out.
    Power,
}

impl Area {
    /// Every area, in the order surfaces show them.
    pub const ALL: [Self; 10] = [
        Self::Apps,
        Self::Settings,
        Self::Network,
        Self::Sound,
        Self::Screen,
        Self::Notifications,
        Self::Storage,
        Self::Devices,
        Self::Power,
        Self::Help,
    ];

    /// Stable id (`apps`, `settings`, …).
    pub const fn id(self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::Apps => "apps",
            Self::Sound => "sound",
            Self::Network => "network",
            Self::Screen => "screen",
            Self::Settings => "settings",
            Self::Notifications => "notifications",
            Self::Storage => "storage",
            Self::Devices => "devices",
            Self::Power => "power",
        }
    }

    /// The setting that switches the area (`assistant.reach.<id>`); [`Area::Help`] has
    /// none: explaining is never switched off.
    pub fn setting(self) -> Option<String> {
        (self != Self::Help).then(|| format!("{REACH_PREFIX}{}", self.id()))
    }

    /// The area a setting key switches.
    pub fn of_setting(key: &str) -> Option<Self> {
        let id = key.strip_prefix(REACH_PREFIX)?;
        Self::ALL
            .into_iter()
            .find(|a| a.id() == id && *a != Self::Help)
    }
}

/// Start of the settings that switch areas.
pub const REACH_PREFIX: &str = "assistant.reach.";
/// Settings the helper never changes itself: its own switches and reach.
pub const OWN_SETTINGS: &str = "assistant.";
/// The setting that lets the helper act at all; off: it only looks and explains.
pub const ACT_KEY: &str = "assistant.act";

/// How much a tool can change.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolRisk {
    /// It only looks.
    Reads,
    /// It changes something that is changed back as easily.
    Changes,
    /// The person confirms it in the system dialog first.
    Confirms,
}

/// One tool of an area, for the surface that lists them.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ToolInfo {
    /// Stable name.
    pub name: String,
    /// How much it can change.
    pub risk: ToolRisk,
}

/// One area with its switch and tools.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AreaReach {
    /// The area.
    pub area: Area,
    /// The person lets the helper act here.
    pub allowed: bool,
    /// Its tools.
    pub tools: Vec<ToolInfo>,
}

/// `agent.reach` – what the helper reaches now (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Reach {
    /// The helper may act at all (`assistant.act`); otherwise it only looks and explains.
    pub acts: bool,
    /// Every area, in [`Area::ALL`] order.
    pub areas: Vec<AreaReach>,
}

/// Most entries [`Activity`] holds.
pub const MAX_ACTIVITY: usize = 50;

/// One request and what came of it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// Seconds since the Unix epoch when it was answered.
    pub at: u64,
    /// What the person asked (data).
    pub request: String,
    /// Where the helper thought.
    pub brain: Brain,
    /// What it did, in order.
    pub steps: Vec<Step>,
    /// What it answered.
    pub said: Text,
}

/// `agent.activity` – what the helper did lately, oldest first (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Activity {
    /// The entries, at most [`MAX_ACTIVITY`].
    pub entries: Vec<Entry>,
}

/// `agent.clear_activity` – forget what the helper did (the Settings app only).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClearActivity {}

/// Answer to [`ClearActivity`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Cleared {
    /// Nothing is kept any more.
    pub ok: bool,
}

/// `agent.progress` – the request being answered moves on (not retained): the surface
/// that asked shows each step as it happens instead of only the end.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum Progress {
    /// A step was taken (done, failed or refused).
    Step(Step),
    /// This tool waits for the person's confirmation in the system dialog.
    Waiting(String),
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(Reach, Activity, ClearActivity, Cleared, Progress);

impl Topic for Progress {
    const TOPIC: &'static str = "agent.progress";
}

impl Topic for Reach {
    const TOPIC: &'static str = "agent.reach";
    const RETAINED: bool = true;
}

impl Topic for Activity {
    const TOPIC: &'static str = "agent.activity";
    const RETAINED: bool = true;
}

impl Call for ClearActivity {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "clear_activity";
    type Reply = Cleared;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_area_but_help_has_its_own_switch() {
        let mut keys: Vec<String> = Area::ALL.into_iter().filter_map(Area::setting).collect();
        assert_eq!(keys.len(), Area::ALL.len() - 1);
        for key in &keys {
            assert!(key.starts_with(OWN_SETTINGS), "the helper cannot set {key}");
            assert_eq!(
                Area::of_setting(key).and_then(Area::setting).as_ref(),
                Some(key)
            );
        }
        keys.dedup();
        assert_eq!(keys.len(), Area::ALL.len() - 1);
        assert_eq!(Area::of_setting("assistant.reach.help"), None);
        assert_eq!(Area::of_setting("assistant.enabled"), None);
    }
}
