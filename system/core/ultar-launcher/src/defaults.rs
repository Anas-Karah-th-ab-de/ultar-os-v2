//! Default apps (`apps.default.*`) and the autostart list (`apps.autostart`): the launcher
//! reads them at start and follows `settings.changed` ([`crate::follow`]). [`Launcher::resolve`]
//! turns a URI into the installed app that opens it (an invalid value falls back to the system
//! default); [`Launcher::candidates`] lists the installed apps that declare a kind, which the
//! Settings page offers as the choices of `apps.default.*`.

use crate::appid::AppId;
use crate::launcher::{lock, Core, Launcher};
use std::collections::{BTreeMap, BTreeSet};
use ultar_bus::messages::launcher::{Kind, MAX_CANDIDATES, MAX_URI};
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::settings::SettingValue;

/// The setting that lists the apps started after sign-in.
pub const AUTOSTART: &str = "apps.autostart";
/// Most apps the autostart list holds (the settings schema's bound).
pub const MAX_AUTOSTART: usize = 64;
/// Prefix of the default-app settings keys.
const PREFIX: &str = "apps.default.";

/// The kind of thing a URI names (shared with the openers, `ultar_bus::messages::launcher`).
pub use ultar_bus::messages::launcher::kind_of_uri;

/// The kind of a default-app settings key, if it is one.
pub fn kind_of_key(key: &str) -> Option<Kind> {
    Kind::from_word(key.strip_prefix(PREFIX)?)
}

/// A stored default app: a valid app id, or the kind's system default.
fn valid_app(text: &str, kind: Kind) -> String {
    AppId::new(text)
        .map(|id| id.as_str().to_owned())
        .unwrap_or_else(|_| kind.default_app().to_owned())
}

/// The default app of each kind (the settings schema's defaults until a setting is read).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defaults {
    apps: BTreeMap<Kind, String>,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            apps: Kind::ALL
                .into_iter()
                .map(|k| (k, k.default_app().to_owned()))
                .collect(),
        }
    }
}

impl Defaults {
    /// Takes a changed `apps.default.*`; `true` when it changed the table. A value that is
    /// no app id falls back to the kind's system default.
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) -> bool {
        let Some(kind) = kind_of_key(key) else {
            return false;
        };
        let SettingValue::Text(text) = value else {
            return false;
        };
        let app = valid_app(text, kind);
        if self.apps.get(&kind) == Some(&app) {
            return false;
        }
        self.apps.insert(kind, app);
        true
    }

    /// The app stored for a kind.
    pub fn app(&self, kind: Kind) -> &str {
        self.apps
            .get(&kind)
            .map(String::as_str)
            .unwrap_or_else(|| kind.default_app())
    }
}

/// The autostart list and what was started this session (each app once after a sign-in).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Autostart {
    list: Vec<String>,
    started: BTreeSet<String>,
    signed_in: bool,
}

impl Autostart {
    /// Takes a changed `apps.autostart`; `true` when the list changed. Entries that are no
    /// app id are dropped; at most [`MAX_AUTOSTART`] are kept.
    pub fn set_list(&mut self, value: &SettingValue) -> bool {
        let SettingValue::List(items) = value else {
            return false;
        };
        let list: Vec<String> = items
            .iter()
            .filter(|a| AppId::new(a).is_ok())
            .take(MAX_AUTOSTART)
            .cloned()
            .collect();
        if list == self.list {
            return false;
        }
        self.list = list;
        true
    }

    /// The apps the list names (in order).
    pub fn list(&self) -> &[String] {
        &self.list
    }

    /// The apps to start now: the whole list once when someone signs in (the person in
    /// front appears), nothing while the session runs, and a fresh start after the next
    /// sign-in once everyone signed out. Records what it hands out.
    pub fn on_session(&mut self, state: &SessionState) -> Vec<String> {
        if state.user.is_none() {
            self.signed_in = false;
            self.started.clear();
            return Vec::new();
        }
        if self.signed_in {
            return Vec::new();
        }
        self.signed_in = true;
        self.list
            .iter()
            .filter(|app| self.started.insert((*app).clone()))
            .cloned()
            .collect()
    }
}

/// Whether the installed app `app` declares the URI's scheme in its manifest. `file` and
/// `files` both mean "opens files" (a mounted drive and a place of the user).
fn declares(core: &Core, app: &str, scheme: &str) -> bool {
    let Ok(id) = AppId::new(app) else {
        return false;
    };
    if core.installed(&id).is_err() {
        return false;
    }
    let wanted: &[&str] = if scheme == "file" || scheme == "files" {
        &["file", "files"]
    } else {
        std::slice::from_ref(&scheme)
    };
    core.manifest(&id)
        .is_ok_and(|m| m.open_schemes.iter().any(|s| wanted.contains(&s.as_str())))
}

impl Launcher {
    /// Takes a settings change (read at start and on `settings.changed`): a default app,
    /// the autostart list or the downloads clean-up ([`crate::clean`]). An invalid value
    /// falls back to the default ([`Defaults`]).
    pub fn follow_setting(&self, key: &str, value: &SettingValue) {
        let mut core = lock(&self.core);
        if key == AUTOSTART {
            core.autostart.set_list(value);
        } else if core.look.on_changed(key, value) {
            // the look of the next apps it starts
        } else if !core.clean.on_changed(key, value) {
            core.defaults.on_changed(key, value);
        }
    }

    /// The autostart apps a session state makes due (the list once after a sign-in, each
    /// app once); records them so the next call while signed in hands out nothing.
    pub fn autostart_due(&self, state: &SessionState) -> Vec<String> {
        lock(&self.core).autostart.on_session(state)
    }

    /// The default app for `uri`: the app of its kind from the settings, kept only when it
    /// is installed and declares the URI's scheme; else the system default; `None` when no
    /// installed app declares it. The caller starts the app; the launcher only resolves.
    pub fn resolve(&self, uri: &str) -> Option<String> {
        if uri.is_empty() || uri.len() > MAX_URI || uri.chars().any(char::is_control) {
            return None;
        }
        let kind = kind_of_uri(uri)?;
        let scheme = uri.split_once(':').map(|(s, _)| s)?;
        let core = lock(&self.core);
        let chosen = core.defaults.app(kind).to_owned();
        if declares(&core, &chosen, scheme) {
            return Some(chosen);
        }
        let fallback = kind.default_app();
        declares(&core, fallback, scheme).then(|| fallback.to_owned())
    }

    /// The installed apps that declare `kind` (their manifests list one of its schemes),
    /// by id, at most [`MAX_CANDIDATES`].
    pub fn candidates(&self, kind: Kind) -> Vec<String> {
        let core = lock(&self.core);
        let mut apps: Vec<String> = core
            .store
            .state
            .apps()
            .filter(|(_, e)| e.installed)
            .filter_map(|(id, _)| {
                let manifest = core.manifest(id).ok()?;
                let declares = kind
                    .schemes()
                    .iter()
                    .any(|s| manifest.open_schemes.iter().any(|d| d == s));
                declares.then(|| id.to_string())
            })
            .take(MAX_CANDIDATES)
            .collect();
        apps.sort();
        apps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_name_their_kind() {
        for (uri, kind) in [
            ("https://example.org/a", Kind::Web),
            ("http://example.org", Kind::Web),
            ("mailto:a@example.org", Kind::Mail),
            ("calendar://event/1", Kind::Calendar),
            ("webcal://example.org/c.ics", Kind::Calendar),
            ("file:///m/u/song.mp3", Kind::Music),
            ("file:///m/u/film.mkv", Kind::Video),
            ("file:///m/u/pic.JPG", Kind::Photos),
            ("file:///m/u/doc.pdf", Kind::Documents),
            ("file:///m/u/page.html", Kind::Documents),
        ] {
            assert_eq!(kind_of_uri(uri), Some(kind), "{uri}");
        }
        for uri in [
            "trash:///",
            "settings://network",
            "file:///m/u/folder",
            "file:///m/u/archive.zip",
            "no-scheme",
            "",
        ] {
            assert_eq!(kind_of_uri(uri), None, "{uri}");
        }
        assert_eq!(kind_of_key("apps.default.web"), Some(Kind::Web));
        assert_eq!(kind_of_key("apps.default.nope"), None);
        assert_eq!(kind_of_key("apps.autostart"), None);
    }

    #[test]
    fn defaults_start_at_the_schema_and_follow_valid_values() {
        let mut d = Defaults::default();
        assert_eq!(d.app(Kind::Web), "browser");
        assert_eq!(d.app(Kind::Music), "media");
        assert!(!d.on_changed("apps.default.web", &SettingValue::Text("browser".into())));
        assert!(d.on_changed("apps.default.web", &SettingValue::Text("acme.web".into())));
        assert_eq!(d.app(Kind::Web), "acme.web");
        // an invalid value falls back to the system default
        assert!(d.on_changed("apps.default.web", &SettingValue::Text("Not An Id!".into())));
        assert_eq!(d.app(Kind::Web), "browser");
        // a value of the wrong type changes nothing
        assert!(!d.on_changed("apps.default.web", &SettingValue::Flag(true)));
        assert!(!d.on_changed("appearance.theme", &SettingValue::Text("dark".into())));
    }

    #[test]
    fn autostart_starts_each_app_once_per_sign_in() {
        let mut a = Autostart::default();
        assert!(a.set_list(&SettingValue::List(vec!["browser".into(), "mail".into()])));
        assert!(!a.set_list(&SettingValue::List(vec!["browser".into(), "mail".into()])));
        let signed_in = SessionState {
            user: Some("amal".into()),
            signed_in: vec!["amal".into()],
        };
        assert_eq!(a.on_session(&signed_in), ["browser", "mail"]);
        // still signed in: nothing again
        assert!(a.on_session(&signed_in).is_empty());
        // a switch keeps the session: nothing again
        let switched = SessionState {
            user: Some("badr".into()),
            signed_in: vec!["amal".into(), "badr".into()],
        };
        assert!(a.on_session(&switched).is_empty());
        // everyone out, then a fresh sign-in starts them again
        assert!(a.on_session(&SessionState::default()).is_empty());
        assert_eq!(a.on_session(&signed_in), ["browser", "mail"]);
        // invalid entries are dropped
        assert!(a.set_list(&SettingValue::List(vec!["browser".into(), "Bad Id".into()])));
        assert!(a.on_session(&SessionState::default()).is_empty());
        assert_eq!(a.on_session(&signed_in), ["browser"]);
    }
}
