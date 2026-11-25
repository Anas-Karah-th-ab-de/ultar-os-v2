//! The on-screen keyboard (`ultar-osk`) without drawing: when it is shown, what a key does,
//! the word being typed, suggestions and autocorrect. Layouts, pages, long-press variants and
//! field kinds come from `ultar-ime`, the same data the phone shell uses.
//!
//! Settings it follows: `keyboard.on_screen` (automatic / always / never),
//! `accessibility.on_screen_keyboard` (always, whatever the mode), `keyboard.suggestions`,
//! `keyboard.autocorrect`, and the languages of `keyboard.input_languages`.

pub mod view;
pub mod words;

use ultar_bus::messages::settings::SettingValue;
use ultar_ime::{Act, Lang, Page, Purpose, Row, Shift};

/// `keyboard.on_screen`.
pub const MODE: &str = "keyboard.on_screen";
/// `accessibility.on_screen_keyboard`.
pub const ACCESSIBILITY: &str = "accessibility.on_screen_keyboard";
/// `keyboard.suggestions`.
pub const SUGGESTIONS: &str = "keyboard.suggestions";
/// `keyboard.autocorrect`.
pub const AUTOCORRECT: &str = "keyboard.autocorrect";
/// `keyboard.input_languages`.
pub const LANGUAGES: &str = "keyboard.input_languages";
/// Every setting the keyboard reads.
pub const FOLLOWED: [&str; 5] = [MODE, ACCESSIBILITY, SUGGESTIONS, AUTOCORRECT, LANGUAGES];

/// When the keyboard appears (`keyboard.on_screen`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Only without a hardware keyboard.
    Automatic,
    /// Whenever a text field is focused.
    Always,
    /// Never (unless the accessibility switch asks for it).
    Never,
}

/// What the keyboard follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OskSettings {
    /// `keyboard.on_screen`.
    pub mode: Mode,
    /// `accessibility.on_screen_keyboard`: shown with every text field.
    pub accessibility: bool,
    /// `keyboard.suggestions`.
    pub suggestions: bool,
    /// `keyboard.autocorrect`.
    pub autocorrect: bool,
    /// The languages the globe key cycles through (first = start).
    pub languages: Vec<Lang>,
}

impl Default for OskSettings {
    fn default() -> OskSettings {
        OskSettings {
            mode: Mode::Automatic,
            accessibility: false,
            suggestions: true,
            autocorrect: false,
            languages: vec![Lang::Ar, Lang::En],
        }
    }
}

impl OskSettings {
    /// Takes a changed setting; `true` when it was one of ours with a valid value (a wrong
    /// type or an unknown word keeps the last good value).
    pub fn apply(&mut self, key: &str, value: &SettingValue) -> bool {
        match (key, value) {
            (MODE, SettingValue::Text(word)) => {
                self.mode = match word.as_str() {
                    "always" => Mode::Always,
                    "never" => Mode::Never,
                    _ => Mode::Automatic,
                };
            }
            (ACCESSIBILITY, SettingValue::Flag(on)) => self.accessibility = *on,
            (SUGGESTIONS, SettingValue::Flag(on)) => self.suggestions = *on,
            (AUTOCORRECT, SettingValue::Flag(on)) => self.autocorrect = *on,
            (LANGUAGES, SettingValue::List(list)) => self.languages = languages(list),
            (LANGUAGES, SettingValue::Text(list)) => {
                self.languages = languages(&list.split(',').map(str::to_owned).collect::<Vec<_>>())
            }
            _ => return false,
        }
        true
    }
}

/// The keyboard languages of a `keyboard.input_languages` list (layout names such as
/// `ar`, `en-us`, `de`; others are skipped). Empty: Arabic and English.
pub fn languages(list: &[String]) -> Vec<Lang> {
    let mut out = Vec::new();
    for name in list.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        let lang = match name.split(['-', '_']).next().unwrap_or_default() {
            "ar" => Lang::Ar,
            "en" | "us" | "gb" => Lang::En,
            "de" => Lang::De,
            _ => continue,
        };
        if !out.contains(&lang) {
            out.push(lang);
        }
    }
    if out.is_empty() {
        out = vec![Lang::Ar, Lang::En];
    }
    out
}

/// A key the keyboard sends that is not text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Special {
    /// Delete the character before the cursor.
    Backspace,
    /// The field's action (new line, send, search …).
    Enter,
}

/// What the keyboard sends to the focused field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// Text typed.
    Commit(String),
    /// A key.
    Key(Special),
}

/// The keyboard's state.
#[derive(Debug, Clone)]
pub struct Keyboard {
    /// What it follows.
    pub settings: OskSettings,
    /// The language shown.
    pub lang: Lang,
    /// Letters or one of the symbol pages.
    pub page: Page,
    /// Shift (Latin only).
    pub shift: Shift,
    /// The kind of the focused field.
    pub purpose: Purpose,
    /// A text field is focused.
    pub active: bool,
    /// A hardware keyboard is attached (`automatic` then keeps the keyboard hidden).
    pub hardware: bool,
    /// The word typed so far (for suggestions and autocorrect).
    pub word: String,
}

impl Default for Keyboard {
    fn default() -> Keyboard {
        Keyboard::new(OskSettings::default())
    }
}

impl Keyboard {
    /// A keyboard for `settings`, hidden, in its first language.
    pub fn new(settings: OskSettings) -> Keyboard {
        let lang = settings.languages.first().copied().unwrap_or(Lang::Ar);
        Keyboard {
            settings,
            lang,
            page: Page::Letters,
            shift: Shift::Off,
            purpose: Purpose::Text,
            active: false,
            hardware: false,
            word: String::new(),
        }
    }

    /// Shown now.
    pub fn visible(&self) -> bool {
        self.active
            && (self.settings.accessibility
                || match self.settings.mode {
                    Mode::Always => true,
                    Mode::Never => false,
                    Mode::Automatic => !self.hardware,
                })
    }

    /// The compositor told whether a text field is focused and its kind.
    pub fn ime_state(&mut self, active: bool, purpose: u8) {
        let purpose = Purpose::from_code(purpose);
        if !active || purpose != self.purpose {
            self.page = Page::Letters;
            self.shift = Shift::Off;
        }
        self.active = active;
        self.purpose = purpose;
        self.word.clear();
    }

    /// A setting changed: the shown language stays when it is still in the list.
    pub fn setting(&mut self, key: &str, value: &SettingValue) -> bool {
        let taken = self.settings.apply(key, value);
        if taken && !self.settings.languages.contains(&self.lang) {
            self.lang = self.settings.languages.first().copied().unwrap_or(Lang::Ar);
        }
        taken
    }

    /// The rows of the page shown.
    pub fn rows(&self) -> Vec<Row> {
        ultar_ime::rows(self.lang, self.page, self.purpose, self.shift, false)
    }

    /// The suggestions for the word typed (none in password and PIN fields).
    pub fn suggestions(&self) -> Vec<&'static str> {
        if !self.settings.suggestions || self.purpose.sensitive() {
            return Vec::new();
        }
        words::suggest(self.lang, &self.word)
    }

    /// A key was pressed.
    pub fn press(&mut self, act: &Act) -> Vec<Out> {
        match act {
            Act::Text(text) => self.text(text),
            Act::Space => {
                let mut out = self.autocorrect();
                out.push(Out::Commit(" ".to_owned()));
                self.word.clear();
                out
            }
            Act::Backspace => {
                self.word.pop();
                vec![Out::Key(Special::Backspace)]
            }
            Act::Action => {
                self.word.clear();
                vec![Out::Key(Special::Enter)]
            }
            Act::Shift => {
                self.shift = match self.shift {
                    Shift::Off => Shift::Once,
                    Shift::Once => Shift::Lock,
                    Shift::Lock => Shift::Off,
                };
                Vec::new()
            }
            Act::ToSymbols => self.page_to(Page::Symbols),
            Act::ToSymbols2 => self.page_to(Page::Symbols2),
            Act::ToLetters => self.page_to(Page::Letters),
            Act::Lang => {
                self.lang = self.lang_after(self.lang);
                self.shift = Shift::Off;
                self.word.clear();
                Vec::new()
            }
        }
    }

    /// A suggestion was chosen: it replaces the word typed, followed by a space.
    pub fn choose(&mut self, suggestion: &str) -> Vec<Out> {
        let mut out = self.erase_word();
        out.push(Out::Commit(format!("{suggestion} ")));
        self.word.clear();
        out
    }

    fn text(&mut self, text: &str) -> Vec<Out> {
        if text.chars().all(char::is_alphabetic) {
            self.word.push_str(text);
        } else {
            self.word.clear();
        }
        if self.shift == Shift::Once {
            self.shift = Shift::Off;
        }
        vec![Out::Commit(text.to_owned())]
    }

    fn page_to(&mut self, page: Page) -> Vec<Out> {
        self.page = page;
        Vec::new()
    }

    fn lang_after(&self, lang: Lang) -> Lang {
        let list = &self.settings.languages;
        match list.iter().position(|l| *l == lang) {
            Some(i) => list[(i + 1) % list.len()],
            None => list.first().copied().unwrap_or(lang),
        }
    }

    /// With autocorrect on, the word typed is replaced by its correction (not in password,
    /// PIN, address or number fields).
    fn autocorrect(&mut self) -> Vec<Out> {
        let plain = !self.purpose.sensitive()
            && !matches!(self.purpose, Purpose::Email | Purpose::Url)
            && !self.purpose.pad();
        if !self.settings.autocorrect || !plain {
            return Vec::new();
        }
        let Some(fixed) = words::correct(self.lang, &self.word) else {
            return Vec::new();
        };
        let mut out = self.erase_word();
        out.push(Out::Commit(fixed));
        out
    }

    /// Backspaces for every character of the word typed.
    fn erase_word(&self) -> Vec<Out> {
        self.word
            .chars()
            .map(|_| Out::Key(Special::Backspace))
            .collect()
    }
}

#[cfg(test)]
mod tests;
