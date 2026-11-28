//! What the service remembers: the apps from other systems it registered, what made
//! them and what the person granted – and the launcher manifest each one gets.

use serde::{Deserialize, Serialize};
use std::path::Path;
use ultar_bus::messages::foreign::{Choices, ForeignApp, NetworkChoice, Source};

/// Prefix of every id (`foreign.<source>-<name>`, see the launcher's `foreign.rs`).
pub const PREFIX: &str = "foreign.";
/// Longest launcher app id.
const MAX_ID: usize = 28;

/// One app from another system.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AppRecord {
    /// Launcher id.
    pub id: String,
    /// Name from its entry.
    pub name: String,
    /// Names by language.
    pub names: Vec<(String, String)>,
    /// Source word (`deb`).
    pub source: String,
    /// The package that brought it.
    pub package: String,
    /// Its entry file's name (without `.desktop`).
    pub entry: String,
    /// The package's version (as the package manager writes it).
    pub version: String,
    /// The symbol's path (or empty).
    pub icon: String,
    /// The program inside the Linux area.
    pub exec: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// What the person granted.
    pub network: bool,
    /// The local network as well.
    pub lan: bool,
    /// The person's folders.
    pub folders: Vec<String>,
    /// Android apps: SHA-256 fingerprints of the signers' certificates (an update must
    /// share one of them).
    #[serde(default)]
    pub signers: Vec<String>,
    /// Android apps: the version number (an update never goes back).
    #[serde(default)]
    pub version_code: u32,
}

/// Everything remembered.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The apps, by id.
    pub apps: Vec<AppRecord>,
}

impl State {
    /// Reads the state at `path` (missing: empty; damaged: an error).
    pub fn load(path: &Path) -> std::io::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// Writes it (`0600`, atomically).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let text = toml::to_string(self).map_err(|_| std::io::ErrorKind::InvalidData)?;
        let tmp = path.with_extension("tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    }

    /// The app with `id`.
    pub fn get(&self, id: &str) -> Option<&AppRecord> {
        self.apps.iter().find(|a| a.id == id)
    }

    /// Puts `record` in place of the app with its id (or adds it).
    pub fn put(&mut self, record: AppRecord) {
        self.apps.retain(|a| a.id != record.id);
        self.apps.push(record);
        self.apps.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// Whether another app than `except` still comes from `package`.
    pub fn package_used(&self, package: &str, except: &str) -> bool {
        self.apps
            .iter()
            .any(|a| a.package == package && a.id != except)
    }

    /// A free id for the entry `file` of `source`, keeping one this package already has.
    pub fn id_for(&self, source: Source, package: &str, file: &str) -> Option<String> {
        let head = format!("{PREFIX}{}-", source.word());
        let base = slug(file, MAX_ID - head.len() - 2)?;
        for n in 1..10 {
            let id = if n == 1 {
                format!("{head}{base}")
            } else {
                format!("{head}{base}-{n}")
            };
            match self.get(&id) {
                None => return Some(id),
                Some(a) if a.package == package && a.entry == file => return Some(id),
                Some(_) => continue,
            }
        }
        None
    }
}

impl AppRecord {
    /// What the person granted, in the bus form.
    pub fn choices(&self) -> Choices {
        Choices {
            network: if self.network {
                NetworkChoice::Internet
            } else {
                NetworkChoice::None
            },
            lan: self.lan,
            folders: self.folders.clone(),
        }
    }

    /// Sets what the person granted.
    pub fn set_choices(&mut self, choices: &Choices) {
        self.network = choices.network == NetworkChoice::Internet;
        self.lan = self.network && choices.lan;
        self.folders = choices.folders.clone();
    }

    /// The bus form of the app.
    pub fn listed(&self) -> ForeignApp {
        ForeignApp {
            id: self.id.clone(),
            name: self.name.clone(),
            names: self.names.clone(),
            source: Source::from_word(&self.source).unwrap_or(Source::Unknown),
            package: self.package.clone(),
            version: self.version.clone(),
            icon: self.icon.clone(),
            choices: self.choices(),
        }
    }

    /// The launcher manifest (kind `compat-deb`, launcher `foreign.rs`).
    pub fn manifest(&self) -> String {
        let mut root = toml::Table::new();
        let text = |s: &str| toml::Value::String(s.to_owned());
        root.insert("id".into(), text(&self.id));
        root.insert(
            "version".into(),
            text(&crate::desktop::launcher_version(&self.version)),
        );
        let kind = if self.source == "apk" {
            "compat-apk"
        } else {
            "compat-deb"
        };
        root.insert("kind".into(), text(kind));
        root.insert("exec".into(), text(&self.exec));
        root.insert(
            "args".into(),
            toml::Value::Array(self.args.iter().map(|a| text(a)).collect()),
        );
        root.insert("name".into(), text(&self.name));
        root.insert("package".into(), text(&self.package));
        let mut perms = toml::Table::new();
        let network = if self.network { "allowed" } else { "none" };
        perms.insert("network".into(), text(network));
        perms.insert(
            "files".into(),
            toml::Value::Array(
                self.folders
                    .iter()
                    .map(|f| text(&format!("{f}:rw")))
                    .collect(),
            ),
        );
        root.insert("permissions".into(), toml::Value::Table(perms));
        let mut foreign = toml::Table::new();
        foreign.insert("lan".into(), toml::Value::Boolean(self.network && self.lan));
        root.insert("foreign".into(), toml::Value::Table(foreign));
        toml::to_string(&root).unwrap_or_default()
    }
}

/// An id part from an entry's file name (`org.xfce.mousepad` → `mousepad`): lower-case
/// letters, digits and single `-`, starting with a letter, at most `max` bytes.
pub fn slug(file: &str, max: usize) -> Option<String> {
    let last = file.rsplit('.').next().unwrap_or(file);
    let mut out = String::new();
    for ch in last.chars().flat_map(char::to_lowercase) {
        let c = if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            ch
        } else {
            '-'
        };
        if c == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        out.push(c);
    }
    let mut out: String = out.trim_matches('-').chars().take(max).collect();
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        return None;
    }
    if !out.starts_with(|c: char| c.is_ascii_lowercase()) {
        out.insert(0, 'a');
        out.truncate(max);
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, package: &str) -> AppRecord {
        AppRecord {
            id: id.into(),
            name: "Mousepad".into(),
            names: Vec::new(),
            source: "deb".into(),
            package: package.into(),
            entry: "org.xfce.mousepad".into(),
            version: "1:0.6.2~rc1-1".into(),
            icon: String::new(),
            exec: "/usr/bin/mousepad".into(),
            args: vec!["--new".into()],
            network: true,
            lan: false,
            folders: vec!["documents".into()],
            signers: Vec::new(),
            version_code: 0,
        }
    }

    #[test]
    fn ids_are_short_valid_and_free() {
        assert_eq!(slug("org.xfce.mousepad", 14).as_deref(), Some("mousepad"));
        assert_eq!(slug("7zip_GUI", 14).as_deref(), Some("a7zip-gui"));
        assert_eq!(
            slug("a-very-long-program-name", 14).as_deref(),
            Some("a-very-long-pr")
        );
        assert_eq!(slug("---", 14), None);
        let mut s = State::default();
        let id = s
            .id_for(Source::Deb, "mousepad", "org.xfce.mousepad")
            .unwrap();
        assert_eq!(id, "foreign.deb-mousepad");
        assert!(id.len() <= MAX_ID);
        s.put(record(&id, "other-package"));
        assert_eq!(
            s.id_for(Source::Deb, "mousepad", "org.xfce.mousepad")
                .as_deref(),
            Some("foreign.deb-mousepad-2")
        );
    }

    #[test]
    fn the_manifest_is_what_the_launcher_reads() {
        let m = record("foreign.deb-mousepad", "mousepad").manifest();
        let t: toml::Table = toml::from_str(&m).unwrap();
        assert_eq!(t["kind"].as_str(), Some("compat-deb"));
        assert_eq!(t["version"].as_str(), Some("1.0.6.2.rc1-1"));
        assert_eq!(t["permissions"]["network"].as_str(), Some("allowed"));
        assert_eq!(
            t["permissions"]["files"].as_array().unwrap()[0].as_str(),
            Some("documents:rw")
        );
        assert!(!t.contains_key("bus"));
    }

    #[test]
    fn state_round_trip() {
        let dir = std::env::temp_dir().join(format!("ufs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.toml");
        assert_eq!(State::load(&path).unwrap(), State::default());
        let mut s = State::default();
        s.put(record("foreign.deb-mousepad", "mousepad"));
        s.save(&path).unwrap();
        assert_eq!(State::load(&path).unwrap(), s.clone());
        assert!(!s.package_used("mousepad", "foreign.deb-mousepad"));
        std::fs::write(&path, "apps = 3").unwrap();
        assert!(State::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
