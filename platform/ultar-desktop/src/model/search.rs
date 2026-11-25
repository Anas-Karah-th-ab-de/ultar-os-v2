//! The overview search (DESKTOP-SPEC §5.1 "Suche"): one field, results grouped – apps,
//! settings (jumps to the panel), calculation, commands, characters. Arabic matches without
//! regard to vowel marks and letter variants (`ultar_i18n::arabic::search_matches`). The
//! characters come from a table that ships with the shell (`model::characters`); sources
//! without a service (files, Store) are not offered.

use super::apps::AppInfo;
use ultar_i18n::{arabic, Translator};

/// The calculator lives in its own module; re-exported so the search and its callers use one
/// name (`search.source.calculator`).
pub use super::calc::calculate;

/// Most results per group.
pub const MAX_PER_GROUP: usize = 6;
/// Longest query in characters.
pub const MAX_QUERY: usize = 200;
/// Shortest query offered to the helper (characters).
pub const MIN_HELPER: usize = 3;

/// A command the search offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    /// Lock the screen.
    Lock,
    /// Open the power menu.
    Power,
    /// Open the display mode panel (Super+P).
    DisplayMode,
    /// Open the quick settings.
    QuickSettings,
}

impl Command {
    /// All commands.
    pub const ALL: [Command; 4] = [
        Command::Lock,
        Command::Power,
        Command::DisplayMode,
        Command::QuickSettings,
    ];

    /// Catalog key of its name (words to match are in the same entry).
    pub fn key(self) -> &'static str {
        match self {
            Command::Lock => "desk.cmd.lock",
            Command::Power => "desk.cmd.power",
            Command::DisplayMode => "desk.cmd.display_mode",
            Command::QuickSettings => "desk.cmd.quick",
        }
    }
}

/// Panels of Settings the search can jump to (DESKTOP-SPEC §8; `settingsd` panels).
pub const SETTING_PANELS: [&str; 10] = [
    "display",
    "appearance",
    "desktop_bar",
    "windows",
    "notifications",
    "power",
    "region",
    "date_time",
    "privacy",
    "account",
];

/// The area of the Settings app that shows a panel (`settings://<area>`): the launcher
/// hands it to Settings as `--area <area>`, which accepts only the ids of its areas.
pub fn panel_area(panel: &str) -> &str {
    match panel {
        "display" => "displays",
        "desktop_bar" => "desktop",
        "date_time" => "datetime",
        "account" => "accounts",
        other => other,
    }
}

/// One result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// Start or raise an app.
    App(String),
    /// Open Settings at a panel.
    Setting(&'static str),
    /// A calculation and its result (digits 0–9).
    Calc(String),
    /// Run a command.
    Command(Command),
    /// A character (emoji, symbol, Arabic letter) from the table; choosing it inserts it.
    Character(String),
    /// A remembered search or opened result; choosing it runs it again.
    History(super::history::Record),
    /// Ask the helper this (the whole query, in the person's words).
    Helper(String),
}

/// What the search looks through: the switches of Settings > Search (`search.source.*`,
/// `search.arabic_variants`). Everything is on until the settings say otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sources {
    /// Installed apps.
    pub apps: bool,
    /// Panels of Settings.
    pub settings: bool,
    /// Calculations.
    pub calculator: bool,
    /// Commands (lock, power, display mode, quick settings).
    pub commands: bool,
    /// Characters (emoji, symbols, Arabic letters) from the shell's table.
    pub characters: bool,
    /// Remembered searches and opened results (`search.history`).
    pub history: bool,
    /// Arabic matches without regard to vowel marks and letter variants.
    pub arabic_variants: bool,
    /// The helper answers on this device (`agent.state` was seen).
    pub helper: bool,
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            apps: true,
            settings: true,
            calculator: true,
            commands: true,
            characters: true,
            history: true,
            arabic_variants: true,
            helper: false,
        }
    }
}

impl Sources {
    /// The settings the search follows.
    pub const KEYS: [&'static str; 7] = [
        "search.source.apps",
        "search.source.settings",
        "search.source.calculator",
        "search.source.commands",
        "search.source.characters",
        "search.history",
        "search.arabic_variants",
    ];

    /// Takes the value of one of [`Sources::KEYS`]; `false` for any other key.
    pub fn set(&mut self, key: &str, on: bool) -> bool {
        let field = match key {
            "search.source.apps" => &mut self.apps,
            "search.source.settings" => &mut self.settings,
            "search.source.calculator" => &mut self.calculator,
            "search.source.commands" => &mut self.commands,
            "search.source.characters" => &mut self.characters,
            "search.history" => &mut self.history,
            "search.arabic_variants" => &mut self.arabic_variants,
            _ => return false,
        };
        *field = on;
        true
    }
}

fn matches(text: &str, query: &str, variants: bool) -> bool {
    (variants && arabic::search_matches(text, query))
        || text.to_lowercase().contains(&query.to_lowercase())
}

/// Searches every source; an empty (or only blank) query has no results.
pub fn search(query: &str, apps: &[AppInfo], tr: &Translator) -> Vec<Hit> {
    search_in(query, apps, tr, Sources::default())
}

/// Searches the sources that are switched on; an empty (or only blank) query has no results.
/// No history is offered (see [`search_in_with_history`] for the shell's own list).
pub fn search_in(query: &str, apps: &[AppInfo], tr: &Translator, sources: Sources) -> Vec<Hit> {
    search_in_with_history(
        query,
        apps,
        tr,
        sources,
        &super::history::History::default(),
    )
}

/// Like [`search_in`], also offering the remembered searches and opened results that match
/// (`search.history`), newest first, before the helper.
pub fn search_in_with_history(
    query: &str,
    apps: &[AppInfo],
    tr: &Translator,
    sources: Sources,
    history: &super::history::History,
) -> Vec<Hit> {
    let query: String = query.chars().take(MAX_QUERY).collect();
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let variants = sources.arabic_variants;
    let mut out = Vec::new();
    if sources.apps {
        out.extend(
            apps.iter()
                .filter(|a| matches(&a.display_name(tr), query, variants) || a.id.contains(query))
                .take(MAX_PER_GROUP)
                .map(|a| Hit::App(a.id.clone())),
        );
    }
    if sources.settings {
        out.extend(
            SETTING_PANELS
                .iter()
                .filter(|p| matches(&tr.t(&format!("desk.search.panel.{p}")), query, variants))
                .take(MAX_PER_GROUP)
                .map(|p| Hit::Setting(p)),
        );
    }
    if let Some(value) = calculate(query).filter(|_| sources.calculator) {
        out.push(Hit::Calc(value));
    }
    if sources.commands {
        out.extend(
            Command::ALL
                .into_iter()
                .filter(|c| {
                    matches(&tr.t(c.key()), query, variants)
                        || matches(&tr.t(&format!("{}.words", c.key())), query, variants)
                })
                .map(Hit::Command),
        );
    }
    out.extend(table_hits(query, variants, sources, history));
    // Always last: whatever was typed can be asked of the helper.
    if sources.helper && query.chars().count() >= MIN_HELPER {
        out.push(Hit::Helper(query.to_owned()));
    }
    out
}

/// The characters and the remembered entries that match (the shell's own tables), each
/// bounded to [`MAX_PER_GROUP`].
fn table_hits(
    query: &str,
    variants: bool,
    sources: Sources,
    history: &super::history::History,
) -> Vec<Hit> {
    let mut out = Vec::new();
    if sources.characters {
        out.extend(
            super::characters::search(query, variants)
                .into_iter()
                .take(MAX_PER_GROUP)
                .map(|c| Hit::Character(c.ch.clone())),
        );
    }
    if sources.history {
        out.extend(
            history
                .matching(query, variants)
                .take(MAX_PER_GROUP)
                .map(|r| Hit::History(r.clone())),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::apps::base_apps;
    use ultar_i18n::Lang;

    #[test]
    fn apps_settings_commands_and_sums_are_found() {
        let ar = crate::i18n::translator(Lang::Ar).unwrap();
        let hits = search("الملفات", &base_apps(), &ar);
        assert_eq!(hits[0], Hit::App("org.ultar.files".into()));
        // Without vowel marks or with them, the same.
        assert!(search("المِلفات", &base_apps(), &ar).contains(&Hit::App("org.ultar.files".into())));
        let en = crate::i18n::translator(Lang::En).unwrap();
        assert!(search("display", &base_apps(), &en).contains(&Hit::Setting("display")));
        assert!(search("lock", &base_apps(), &en).contains(&Hit::Command(Command::Lock)));
        assert!(search("  ", &base_apps(), &en).is_empty());
        assert_eq!(
            search("12*3", &base_apps(), &en),
            vec![Hit::Calc("36".into())]
        );
    }

    #[test]
    fn characters_are_found_by_name_and_the_source_switches_them_off() {
        let en = crate::i18n::translator(Lang::En).unwrap();
        let ar = crate::i18n::translator(Lang::Ar).unwrap();
        let heart = Hit::Character("❤".into());
        assert!(search("heart", &base_apps(), &en).contains(&heart));
        assert!(search("قلب", &base_apps(), &ar).contains(&heart));
        // The source switched off in Settings: no characters.
        let mut sources = Sources::default();
        assert!(sources.set("search.source.characters", false));
        assert!(!search_in("heart", &base_apps(), &en, sources).contains(&heart));
        // Apps still found with characters off.
        assert!(search_in("files", &base_apps(), &en, sources)
            .iter()
            .any(|h| matches!(h, Hit::App(_))));
    }

    #[test]
    fn history_is_offered_when_on_and_gone_when_off() {
        use super::super::history::{History, Record};
        let en = crate::i18n::translator(Lang::En).unwrap();
        let mut history = History::default();
        history.record(Record::Search("weather".into()));
        history.record(Record::App("org.ultar.files".into()));
        let hit = Hit::History(Record::Search("weather".into()));
        let sources = Sources::default();
        assert!(
            search_in_with_history("weather", &base_apps(), &en, sources, &history).contains(&hit)
        );
        // The plain search (no history) does not offer it.
        assert!(!search_in("weather", &base_apps(), &en, sources).contains(&hit));
        // Switched off in Settings: no history hits.
        let mut off = Sources::default();
        assert!(off.set("search.history", false));
        assert!(
            !search_in_with_history("weather", &base_apps(), &en, off, &history).contains(&hit)
        );
    }

    #[test]
    fn a_source_switched_off_in_settings_is_not_searched() {
        let ar = crate::i18n::translator(Lang::Ar).unwrap();
        let en = crate::i18n::translator(Lang::En).unwrap();
        let files = Hit::App("org.ultar.files".into());
        let find = |query: &str, tr: &Translator, key: &str| {
            let mut sources = Sources::default();
            assert!(sources.set(key, false), "{key}");
            search_in(query, &base_apps(), tr, sources)
        };
        assert!(!find("الملفات", &ar, "search.source.apps").contains(&files));
        assert!(find("الملفات", &ar, "search.source.commands").contains(&files));
        assert!(!find("display", &en, "search.source.settings").contains(&Hit::Setting("display")));
        assert!(!find("lock", &en, "search.source.commands").contains(&Hit::Command(Command::Lock)));
        assert!(find("12*3", &en, "search.source.calculator").is_empty());
        // Without the Arabic variants only the exact spelling matches.
        assert!(find("الملفات", &ar, "search.arabic_variants").contains(&files));
        assert!(!find("المِلفات", &ar, "search.arabic_variants").contains(&files));
        // A key the search does not follow is refused (files has no source yet).
        assert!(!Sources::default().set("search.source.files", false));
    }
}
