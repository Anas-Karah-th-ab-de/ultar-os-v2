//! The overview search (DESKTOP-SPEC §5.1 "Suche"): one field, results grouped – apps,
//! settings (jumps to the panel), calculation, commands, characters. Arabic matches without
//! regard to vowel marks and letter variants (`ultar_i18n::arabic::search_matches`). The
//! characters come from a table that ships with the shell (`model::characters`); sources
//! without a service (files, Store) are not offered.

use super::apps::AppInfo;
use ultar_i18n::{arabic, numbers, Translator};

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
            arabic_variants: true,
            helper: false,
        }
    }
}

impl Sources {
    /// The settings the search follows.
    pub const KEYS: [&'static str; 6] = [
        "search.source.apps",
        "search.source.settings",
        "search.source.calculator",
        "search.source.commands",
        "search.source.characters",
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
pub fn search_in(query: &str, apps: &[AppInfo], tr: &Translator, sources: Sources) -> Vec<Hit> {
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
    if sources.characters {
        out.extend(
            super::characters::search(query, variants)
                .into_iter()
                .take(MAX_PER_GROUP)
                .map(|c| Hit::Character(c.ch.to_owned())),
        );
    }
    // Always last: whatever was typed can be asked of the helper.
    if sources.helper && query.chars().count() >= MIN_HELPER {
        out.push(Hit::Helper(query.to_owned()));
    }
    out
}

/// Evaluates `+ - * / × ÷ ( )` on decimal numbers (Arabic-Indic digits and the Arabic
/// decimal comma are accepted); `None` for anything else or a single plain number.
pub fn calculate(text: &str) -> Option<String> {
    let text = numbers::to_western_digits(text)
        .replace('٫', ".")
        .replace('×', "*")
        .replace('÷', "/");
    if !text.chars().any(|c| matches!(c, '+' | '-' | '*' | '/')) {
        return None;
    }
    let tokens: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = Parser {
        t: &tokens,
        i: 0,
        depth: 0,
    };
    let value = p.sum()?;
    if p.i != tokens.len() || !value.is_finite() || value.abs() > 1e15 {
        return None;
    }
    let rounded = (value * 1e9).round() / 1e9;
    let mut s = format!("{rounded}");
    if s == "-0" {
        s = "0".into();
    }
    Some(s)
}

struct Parser<'a> {
    t: &'a [char],
    i: usize,
    depth: u8,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.t.get(self.i).copied()
    }

    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.i += 1;
            let r = self.product()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn product(&mut self) -> Option<f64> {
        let mut v = self.atom()?;
        while let Some(op @ ('*' | '/')) = self.peek() {
            self.i += 1;
            let r = self.atom()?;
            if op == '/' && r == 0.0 {
                return None;
            }
            v = if op == '*' { v * r } else { v / r };
        }
        Some(v)
    }

    fn atom(&mut self) -> Option<f64> {
        match self.peek()? {
            '(' if self.depth < 16 => {
                self.i += 1;
                self.depth += 1;
                let v = self.sum()?;
                self.depth -= 1;
                (self.peek() == Some(')')).then(|| self.i += 1)?;
                Some(v)
            }
            '-' => {
                self.i += 1;
                Some(-self.atom()?)
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.')
                    && self.i - start < 24
                {
                    self.i += 1;
                }
                self.t[start..self.i]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .ok()
            }
            _ => None,
        }
    }
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
    fn the_calculator_is_small_and_safe() {
        assert_eq!(calculate("1+2*3").as_deref(), Some("7"));
        assert_eq!(calculate("(1+2)*3").as_deref(), Some("9"));
        assert_eq!(calculate("١٠ ÷ ٤").as_deref(), Some("2.5"));
        assert_eq!(calculate("-3+1").as_deref(), Some("-2"));
        assert_eq!(calculate("0.1+0.2").as_deref(), Some("0.3"));
        for bad in [
            "42",
            "1/0",
            "1+",
            "(1+2",
            "a+b",
            "1e400*1e400",
            &"(".repeat(40),
            "9999999999999999*9999",
        ] {
            assert_eq!(calculate(bad), None, "{bad}");
        }
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
        assert!(!Sources::default().set("search.history", false));
    }
}
