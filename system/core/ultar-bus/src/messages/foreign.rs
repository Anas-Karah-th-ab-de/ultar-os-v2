//! Contract of the service for apps from other systems (`ultar-foreign`,
//! `docs/FREMD-APPS.md`, `docs/integration/ultar-foreign.md`).
//!
//! A file of the person (`.deb`, later `.exe`/`.msi`/`.apk`) or a package name of
//! the Linux area is installed into the shared, read-only Linux area; every
//! program it brings becomes an app of the profile `app.foreign` in the launcher.
//! What a file says about itself (name, publisher) is **never verified**: the
//! answers carry it as data, and every installation of a file of the person is
//! confirmed with an administrator's password in the system dialog first.
//!
//! Names of apps and packages are data written by others, never interface text.

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "foreign";
/// Longest path of a file to inspect or install.
pub const MAX_PATH: usize = 1024;
/// Longest name, version or publisher text.
pub const MAX_TEXT: usize = 128;
/// Most apps the list answers.
pub const MAX_APPS: usize = 512;
/// Most shared folders of one app.
pub const MAX_FOLDERS: usize = 8;
/// The person's folders an app from another system may be given (the user's file
/// areas of the launcher).
pub const FOLDERS: [&str; 7] = [
    "documents",
    "downloads",
    "desktop",
    "media-pictures",
    "media-music",
    "media-videos",
    "home",
];

/// The service's texts (keys `foreign.*`) by language tag, for the system dialog (the
/// confirmation) and the surfaces that show its jobs.
pub const LOCALES: [(&str, &str); 3] = [
    ("ar", include_str!("../../locales/foreign/ar.ftl")),
    ("en", include_str!("../../locales/foreign/en.ftl")),
    ("de", include_str!("../../locales/foreign/de.ftl")),
];

/// Where an app comes from.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// A Debian/Ubuntu package (`.deb`) or a package of the Linux area.
    Deb,
    /// A Windows program (`.exe`).
    Exe,
    /// A Windows installer (`.msi`).
    Msi,
    /// An Android app (`.apk`).
    Apk,
    /// Not a kind this service knows.
    Unknown,
}

impl Source {
    /// Every source.
    pub const ALL: [Source; 5] = [Self::Deb, Self::Exe, Self::Msi, Self::Apk, Self::Unknown];

    /// Stable word (records, app ids, the launcher's manifest `kind`).
    pub const fn word(self) -> &'static str {
        match self {
            Self::Deb => "deb",
            Self::Exe => "exe",
            Self::Msi => "msi",
            Self::Apk => "apk",
            Self::Unknown => "unknown",
        }
    }

    /// The source a word names.
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.word() == word)
    }

    /// Whether this version of the system can install it (Windows and Android
    /// follow in later stages; until then they are refused with
    /// [`InstallOutcome::NotAvailable`]).
    pub const fn installable(self) -> bool {
        matches!(self, Self::Deb)
    }
}

/// What an app may reach on the network.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NetworkChoice {
    /// Nothing but its own loopback (the default).
    #[default]
    None,
    /// The internet: never the device itself, its other apps or services.
    Internet,
}

/// What the person grants an app at installation (and may change later).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Choices {
    /// Network.
    pub network: NetworkChoice,
    /// With [`NetworkChoice::Internet`]: also the local network (printers, NAS).
    pub lan: bool,
    /// Folders of the person the app may read and write (words of [`FOLDERS`]).
    pub folders: Vec<String>,
}

impl Choices {
    /// Whether every value is one the service accepts.
    pub fn is_valid(&self) -> bool {
        let lan_ok = !self.lan || self.network == NetworkChoice::Internet;
        let mut folders = self.folders.clone();
        folders.sort();
        folders.dedup();
        lan_ok
            && folders.len() == self.folders.len()
            && self.folders.len() <= MAX_FOLDERS
            && self.folders.iter().all(|f| FOLDERS.contains(&f.as_str()))
    }

    /// Whether `self` grants anything `before` did not (network, the local network, a
    /// folder): such a change is confirmed by an administrator like an installation.
    pub fn widens(&self, before: &Choices) -> bool {
        (self.network == NetworkChoice::Internet && before.network == NetworkChoice::None)
            || (self.lan && !before.lan)
            || self.folders.iter().any(|f| !before.folders.contains(f))
    }
}

/// `foreign.inspect` – what a file of the person is, before anything happens.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Inspect {
    /// Absolute path of the file (in one of the person's folders).
    pub path: String,
}

/// What [`Inspect`] found out about a file.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Inspected {
    /// Its kind.
    pub source: Source,
    /// Name the file gives itself (unverified).
    pub name: String,
    /// Version the file gives itself (unverified).
    pub version: String,
    /// Publisher the file names (unverified; empty when it names none).
    pub publisher: String,
    /// Size of the file in bytes.
    pub size_bytes: u64,
    /// Always `false`: nothing in a file of the person is checked by KM Code Craft.
    pub verified: bool,
    /// Whether this version of the system can install it.
    pub installable: bool,
    /// What the dialog offers first (the file's needs, never more than it asks).
    pub suggested: Choices,
}

/// How [`Inspect`] ended.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum InspectOutcome {
    /// The file was read.
    Found(Inspected),
    /// It cannot be read (missing, no access, not a plain file).
    Unreadable,
    /// It is larger than the service reads.
    TooLarge,
    /// The path is not one the service accepts.
    Invalid,
}

/// Answer to [`Inspect`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct InspectReply {
    /// The result.
    pub outcome: InspectOutcome,
}

/// What is installed.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum InstallFrom {
    /// A file of the person (absolute path).
    File(String),
    /// A package of the Linux area's sources by name (verified by the sources'
    /// signatures).
    Archive(String),
}

/// `foreign.install` – installs a file or a package of the Linux area. A file of
/// the person needs an administrator's confirmation in the system dialog; the
/// work runs as a job ([`Progress`]).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Install {
    /// What.
    pub from: InstallFrom,
    /// What the person grants.
    pub choices: Choices,
}

/// How a request that starts a job ended.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstallOutcome {
    /// The job runs; its steps follow on [`Progress`].
    Started(u64),
    /// Windows and Android follow in a later version.
    NotAvailable,
    /// The Linux area is not installed (component „Linux programs“).
    RuntimeMissing,
    /// Another job runs.
    Busy,
    /// A value of the request is refused (path, name, choices).
    Invalid,
}

/// Answer to [`Install`] and [`RuntimeUpdate`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct InstallReply {
    /// The result.
    pub outcome: InstallOutcome,
}

/// Where a job is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// Waiting for the administrator's confirmation.
    Confirm,
    /// Reading the file and preparing the installation cell.
    Prepare,
    /// The installation cell works (packages, downloads of the Linux area).
    Install,
    /// The programs become apps.
    Register,
    /// Finished.
    Done,
    /// The person (or nobody in time) did not confirm.
    Cancelled,
    /// Failed; nothing of it stays.
    Failed(Failure),
}

/// Why a job failed.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failure {
    /// The file is damaged or not what it claims to be.
    Damaged,
    /// The package manager of the Linux area refused it (dependencies, scripts).
    Refused,
    /// The package brings no program with a window.
    NoPrograms,
    /// A download of the Linux area failed.
    Network,
    /// Not enough space.
    Space,
    /// The launcher refused an app (limits, storage).
    Launcher,
    /// The confirmation could not be asked (no system dialog, no administrator).
    NoConfirmation,
    /// Something else; the log has the details.
    Internal,
}

/// Topic `foreign.progress` – one step of a job.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Progress {
    /// The job.
    pub job: u64,
    /// Its step.
    pub step: Step,
    /// Rough progress of the job in percent (0–100).
    pub percent: u8,
    /// With [`Step::Done`]: the ids of the apps it made.
    pub apps: Vec<String>,
}

/// One app from another system in [`ListReply`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ForeignApp {
    /// Its id in the launcher (`foreign.<source>-<name>`).
    pub id: String,
    /// Its name from its own entry (unverified), in the language of the entry
    /// that fits the person best; [`ForeignApp::names`] has the others.
    pub name: String,
    /// Names by language code (`ar`, `en`, `de`) where the entry has them.
    pub names: Vec<(String, String)>,
    /// Where it comes from.
    pub source: Source,
    /// The package that brought it.
    pub package: String,
    /// Its version.
    pub version: String,
    /// Absolute path of its symbol (PNG, readable by everyone), empty if none.
    pub icon: String,
    /// What the person granted.
    pub choices: Choices,
}

/// `foreign.list` – the apps from other systems.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct List {}

/// Answer to [`List`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ListReply {
    /// The apps, by id (at most [`MAX_APPS`]).
    pub apps: Vec<ForeignApp>,
}

/// How a simple change ended.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Done.
    Done,
    /// The change grants more: an administrator is asked first. The end follows on
    /// [`Progress`] of this job (`Done`, `Cancelled` or `Failed`).
    Asking(u64),
    /// No app from another system has this id.
    Unknown,
    /// A value is refused.
    Invalid,
    /// Another job runs.
    Busy,
    /// The change could not be stored or made.
    Failed,
}

/// Answer to [`SetChoices`] and [`Remove`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutcomeReply {
    /// The result.
    pub outcome: Outcome,
}

/// `foreign.set_choices` – what an installed app may reach from its next start.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetChoices {
    /// App id.
    pub id: String,
    /// The new choices.
    pub choices: Choices,
}

/// `foreign.remove` – removes an app (and its package when no other app needs it).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Remove {
    /// App id.
    pub id: String,
    /// Keep its data for a later installation.
    pub keep_data: bool,
}

/// `foreign.runtime_state` – the Linux area.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RuntimeState {}

/// Answer to [`RuntimeState`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct RuntimeStateReply {
    /// The component „Linux programs“ is installed.
    pub installed: bool,
    /// Its version (empty when not installed).
    pub version: String,
    /// Space it takes in bytes.
    pub size_bytes: u64,
    /// A job runs.
    pub busy: bool,
}

/// `foreign.runtime_update` – updates the packages of the Linux area from its
/// sources (in the installation cell; answers with a job like [`Install`]).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RuntimeUpdate {}

/// Topic `foreign.changed` – the list of apps from other systems changed.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Changed {}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    Inspect,
    InspectReply,
    Install,
    InstallReply,
    Progress,
    List,
    ListReply,
    OutcomeReply,
    SetChoices,
    Remove,
    RuntimeState,
    RuntimeStateReply,
    RuntimeUpdate,
    Changed
);

macro_rules! call {
    ($t:ty, $m:literal, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
call!(Inspect, "inspect", InspectReply);
call!(Install, "install", InstallReply);
call!(List, "list", ListReply);
call!(SetChoices, "set_choices", OutcomeReply);
call!(Remove, "remove", OutcomeReply);
call!(RuntimeState, "runtime_state", RuntimeStateReply);
call!(RuntimeUpdate, "runtime_update", InstallReply);

impl Topic for Progress {
    const TOPIC: &'static str = "foreign.progress";
}

impl Topic for Changed {
    const TOPIC: &'static str = "foreign.changed";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    fn round_trip<T: Message + PartialEq + std::fmt::Debug>(value: &T) {
        let bytes = encode(value).unwrap();
        assert_eq!(&decode::<T>(&bytes).unwrap(), value);
    }

    #[test]
    fn messages_round_trip() {
        let choices = Choices {
            network: NetworkChoice::Internet,
            lan: false,
            folders: vec!["documents".into()],
        };
        round_trip(&Install {
            from: InstallFrom::File("/data/ultar/areas/downloads/a.deb".into()),
            choices: choices.clone(),
        });
        round_trip(&InspectReply {
            outcome: InspectOutcome::Found(Inspected {
                source: Source::Deb,
                name: "mousepad".into(),
                version: "0.6.2-1".into(),
                publisher: "Xubuntu Developers".into(),
                size_bytes: 9000,
                verified: false,
                installable: true,
                suggested: Choices::default(),
            }),
        });
        round_trip(&Progress {
            job: 7,
            step: Step::Failed(Failure::Refused),
            percent: 40,
            apps: Vec::new(),
        });
        round_trip(&ListReply {
            apps: vec![ForeignApp {
                id: "foreign.deb-mousepad".into(),
                name: "Mousepad".into(),
                names: vec![("ar".into(), "ماوس باد".into())],
                source: Source::Deb,
                package: "mousepad".into(),
                version: "0.6.2-1".into(),
                icon: "/data/ultar/foreign/icons/foreign.deb-mousepad.png".into(),
                choices,
            }],
        });
        round_trip(&InstallReply {
            outcome: InstallOutcome::Started(3),
        });
    }

    #[test]
    fn texts_exist_in_every_language_with_the_same_keys() {
        let keys = |src: &str| -> Vec<String> {
            let mut k: Vec<String> = src
                .lines()
                .filter(|l| !l.starts_with('#') && l.contains(" = "))
                .map(|l| l.split(" = ").next().unwrap_or_default().to_owned())
                .collect();
            k.sort();
            k
        };
        let ar = keys(LOCALES[0].1);
        assert!(ar.contains(&"foreign.confirm.install".to_owned()));
        for s in Source::ALL {
            assert!(ar.contains(&format!("foreign.source.{}", s.word())));
        }
        for (_, src) in &LOCALES[1..] {
            assert_eq!(keys(src), ar);
        }
    }

    #[test]
    fn sources_and_choices() {
        for s in Source::ALL {
            assert_eq!(Source::from_word(s.word()), Some(s));
        }
        assert!(Source::Deb.installable());
        for later in [Source::Exe, Source::Msi, Source::Apk, Source::Unknown] {
            assert!(!later.installable());
        }
        assert!(Choices::default().is_valid());
        let lan_without_net = Choices {
            lan: true,
            ..Choices::default()
        };
        assert!(!lan_without_net.is_valid());
        let twice = Choices {
            folders: vec!["documents".into(), "documents".into()],
            ..Choices::default()
        };
        assert!(!twice.is_valid());
        let unknown = Choices {
            folders: vec!["/etc".into()],
            ..Choices::default()
        };
        assert!(!unknown.is_valid());
    }

    #[test]
    fn granting_more_is_noticed_and_taking_back_is_not() {
        let none = Choices::default();
        let net = Choices {
            network: NetworkChoice::Internet,
            ..Choices::default()
        };
        let lan = Choices {
            lan: true,
            ..net.clone()
        };
        let docs = Choices {
            folders: vec!["documents".into()],
            ..net.clone()
        };
        assert!(net.widens(&none) && lan.widens(&net) && docs.widens(&net));
        assert!(
            docs.widens(&lan),
            "a folder is more, even with less network"
        );
        for (after, before) in [(&none, &net), (&net, &lan), (&net, &docs), (&none, &none)] {
            assert!(!after.widens(before), "{after:?} after {before:?}");
        }
    }
}
