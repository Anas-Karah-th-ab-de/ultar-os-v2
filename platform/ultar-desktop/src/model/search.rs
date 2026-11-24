//! The overview search (DESKTOP-SPEC §5.1 "Suche"): one field, results grouped – apps,
//! settings (jumps to the panel), calculation, commands. Arabic matches without regard to
//! vowel marks and letter variants (`ultar_i18n::arabic::search_matches`). Sources without a
//! service (files, Store, characters) are not offered.

use super::apps::AppInfo;
use ultar_i18n::{arabic, numbers, Translator};

/// Most results per group.
pub const MAX_PER_GROUP: usize = 6;
/// Longest query in characters.
pub const MAX_QUERY: usize = 200;

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
}

fn matches(text: &str, query: &str) -> bool {
    arabic::search_matches(text, query) || text.to_lowercase().contains(&query.to_lowercase())
}

/// Searches; an empty (or only blank) query has no results.
pub fn search(query: &str, apps: &[AppInfo], tr: &Translator) -> Vec<Hit> {
    let query: String = query.chars().take(MAX_QUERY).collect();
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Hit> = apps
        .iter()
        .filter(|a| matches(&a.display_name(tr), query) || a.id.contains(query))
        .take(MAX_PER_GROUP)
        .map(|a| Hit::App(a.id.clone()))
        .collect();
    out.extend(
        SETTING_PANELS
            .iter()
            .filter(|p| matches(&tr.t(&format!("desk.search.panel.{p}")), query))
            .take(MAX_PER_GROUP)
            .map(|p| Hit::Setting(p)),
    );
    if let Some(value) = calculate(query) {
        out.push(Hit::Calc(value));
    }
    out.extend(
        Command::ALL
            .into_iter()
            .filter(|c| {
                matches(&tr.t(c.key()), query)
                    || matches(&tr.t(&format!("{}.words", c.key())), query)
            })
            .map(Hit::Command),
    );
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
}
