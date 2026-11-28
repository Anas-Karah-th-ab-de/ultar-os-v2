//! Installing an app from another system (`.deb`, `.exe`, `.msi`, `.apk`) from Files: the
//! sheet's state and its steps, without I/O (`docs/FREMD-APPS.md`, contract
//! `ultar_bus::messages::foreign`).
//!
//! Files only shows what the service `foreign` says: the service reads the file in a cell,
//! asks an administrator in the system dialog and installs. Nothing a file says about itself
//! is verified, and the sheet says so; every permission starts **off**.

use crate::fs::{AreaId, Loc};
use crate::model::Drive;
use crate::service::PortError;
use ultar_bus::messages::foreign::{
    Choices, Failure, InspectOutcome, Inspected, InstallOutcome, NetworkChoice, Progress, Source,
    Step,
};

/// File name endings Files hands to the service instead of opening them.
pub const ENDINGS: [&str; 4] = [".deb", ".exe", ".msi", ".apk"];

/// Where the launcher keeps the person's areas on the device (`launcher.toml`
/// `areas_root`): the service takes paths there, Files sees them at `/areas`.
pub const HOST_AREAS: &str = "/data/ultar/areas";

/// The folders the sheet offers, with the label of Files' sidebar (contract word, key).
pub const FOLDERS: [(&str, &str); 7] = [
    ("documents", "files.place.documents"),
    ("downloads", "files.place.downloads"),
    ("media-pictures", "files.place.pictures"),
    ("media-music", "files.place.music"),
    ("media-videos", "files.place.videos"),
    ("desktop", "files.place.desktop"),
    ("home", "files.place.home"),
];

/// Whether Files offers to install an item of this name.
pub fn is_installable_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    ENDINGS.iter().any(|e| lower.ends_with(e) && lower.len() > e.len())
}

/// The path of `loc` as the service sees it (the device's areas or a mounted drive);
/// `None` for the trash and unknown drives.
pub fn host_path(loc: &Loc, drives: &[Drive]) -> Option<String> {
    let base = match &loc.area {
        AreaId::Trash => return None,
        AreaId::Drive(_) => {
            let d = drives.iter().find(|d| d.area == loc.area)?;
            d.uri.strip_prefix("file://")?.trim_end_matches('/').to_owned()
        }
        area => format!("{HOST_AREAS}/{}", area.dir_name()),
    };
    if loc.parts.is_empty() {
        return None;
    }
    Some(format!("{base}/{}", loc.parts.join("/")))
}

/// Where the sheet is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The service reads the file.
    Reading,
    /// What the file is; the person chooses and installs.
    Ready(Inspected),
    /// The installation was asked for; the service answers with its job.
    Starting,
    /// The job runs.
    Running {
        /// The service's job.
        job: u64,
        /// Its step.
        step: Step,
        /// Rough progress (0–100).
        percent: u8,
    },
    /// Installed: the apps it made.
    Done(Vec<String>),
    /// It cannot be installed or failed: i18n key of the reason.
    Problem(&'static str),
}

/// The sheet over the Files window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    /// The item's name as Files shows it.
    pub file: String,
    /// Its path for the service.
    pub path: String,
    /// Where it is.
    pub phase: Phase,
    /// What the person grants (starts with nothing).
    pub choices: Choices,
}

impl Sheet {
    /// A sheet that waits for the service to read `path`.
    pub fn new(file: &str, path: String) -> Sheet {
        Sheet {
            file: file.to_owned(),
            path,
            phase: Phase::Reading,
            choices: Choices::default(),
        }
    }

    /// The service's answer to `foreign.inspect`.
    pub fn inspected(&mut self, answer: Result<InspectOutcome, PortError>) {
        self.phase = match answer {
            Ok(InspectOutcome::Found(i)) if i.installable => Phase::Ready(i),
            Ok(InspectOutcome::Found(i)) => Phase::Problem(not_installable(i.source)),
            Ok(InspectOutcome::Unreadable) => Phase::Problem("files.foreign.unreadable"),
            Ok(InspectOutcome::TooLarge) => Phase::Problem("files.foreign.too_large"),
            Ok(InspectOutcome::Invalid) => Phase::Problem("files.foreign.invalid"),
            Err(e) => Phase::Problem(service_key(e)),
        };
    }

    /// The internet switch (off also takes the local network back).
    pub fn set_internet(&mut self, on: bool) {
        self.choices.network = if on {
            NetworkChoice::Internet
        } else {
            NetworkChoice::None
        };
        if !on {
            self.choices.lan = false;
        }
    }

    /// The local network (only with the internet).
    pub fn set_lan(&mut self, on: bool) {
        self.choices.lan = on && self.choices.network == NetworkChoice::Internet;
    }

    /// One of the person's folders.
    pub fn set_folder(&mut self, word: &str, on: bool) {
        if !FOLDERS.iter().any(|(w, _)| *w == word) {
            return;
        }
        self.choices.folders.retain(|f| f != word);
        if on {
            self.choices.folders.push(word.to_owned());
            self.choices
                .folders
                .sort_by_key(|f| FOLDERS.iter().position(|(w, _)| w == f));
        }
    }

    /// The person pressed "Install".
    pub fn start(&mut self) -> bool {
        if matches!(self.phase, Phase::Ready(_)) && self.choices.is_valid() {
            self.phase = Phase::Starting;
            return true;
        }
        false
    }

    /// The service's answer to `foreign.install`.
    pub fn started(&mut self, answer: Result<InstallOutcome, PortError>) {
        self.phase = match answer {
            Ok(InstallOutcome::Started(job)) => Phase::Running {
                job,
                step: Step::Confirm,
                percent: 0,
            },
            Ok(InstallOutcome::NotAvailable) => Phase::Problem("files.foreign.not_yet"),
            Ok(InstallOutcome::RuntimeMissing) => Phase::Problem("files.foreign.runtime_missing"),
            Ok(InstallOutcome::Busy) => Phase::Problem("files.foreign.busy"),
            Ok(InstallOutcome::Invalid) => Phase::Problem("files.foreign.invalid"),
            Err(e) => Phase::Problem(service_key(e)),
        };
    }

    /// A step of a job; `true` when it was this sheet's job.
    pub fn progress(&mut self, p: &Progress) -> bool {
        let Phase::Running { job, .. } = self.phase else {
            return false;
        };
        if job != p.job {
            return false;
        }
        self.phase = match p.step {
            Step::Done => Phase::Done(p.apps.clone()),
            Step::Cancelled => Phase::Problem("files.foreign.cancelled"),
            Step::Failed(f) => Phase::Problem(failure_key(f)),
            step => Phase::Running {
                job,
                step,
                percent: p.percent.min(100),
            },
        };
        true
    }

    /// The job this sheet follows, while it runs.
    pub fn job(&self) -> Option<u64> {
        match self.phase {
            Phase::Running { job, .. } => Some(job),
            _ => None,
        }
    }
}

/// i18n key of a source's kind ("Linux program").
pub fn kind_key(source: Source) -> &'static str {
    match source {
        Source::Deb => "files.foreign.kind.linux",
        Source::Exe | Source::Msi => "files.foreign.kind.windows",
        Source::Apk => "files.foreign.kind.android",
        Source::Unknown => "files.foreign.kind.unknown",
    }
}

/// i18n key of a step that runs.
pub fn step_key(step: Step) -> &'static str {
    match step {
        Step::Confirm => "files.foreign.step.confirm",
        Step::Prepare => "files.foreign.step.prepare",
        Step::Install => "files.foreign.step.install",
        _ => "files.foreign.step.register",
    }
}

/// i18n key of why a job failed.
pub fn failure_key(f: Failure) -> &'static str {
    match f {
        Failure::Damaged => "files.foreign.failed.damaged",
        Failure::Refused => "files.foreign.failed.refused",
        Failure::NoPrograms => "files.foreign.failed.no_programs",
        Failure::Network => "files.foreign.failed.network",
        Failure::Space => "files.foreign.failed.space",
        Failure::Launcher => "files.foreign.failed.launcher",
        Failure::NoConfirmation => "files.foreign.failed.no_confirmation",
        Failure::Internal => "files.foreign.failed.internal",
    }
}

fn not_installable(source: Source) -> &'static str {
    match source {
        Source::Unknown => "files.foreign.unknown",
        _ => "files.foreign.not_yet",
    }
}

fn service_key(e: PortError) -> &'static str {
    match e {
        PortError::Unavailable => "files.foreign.no_service",
        _ => "files.foreign.refused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(source: Source, installable: bool) -> InspectOutcome {
        InspectOutcome::Found(Inspected {
            source,
            name: "mousepad".into(),
            version: "0.6.2".into(),
            publisher: String::new(),
            size_bytes: 9000,
            verified: false,
            installable,
            suggested: Choices {
                network: NetworkChoice::Internet,
                lan: false,
                folders: vec!["documents".into()],
            },
        })
    }

    #[test]
    fn endings_and_host_paths() {
        assert!(is_installable_name("Mousepad.DEB") && is_installable_name("setup.exe"));
        assert!(!is_installable_name(".deb") && !is_installable_name("a.debx"));
        let loc = Loc::root(AreaId::Downloads).child("a.deb").unwrap();
        assert_eq!(
            host_path(&loc, &[]).as_deref(),
            Some("/data/ultar/areas/downloads/a.deb")
        );
        let usb = Drive {
            id: "sdb1".into(),
            name: "USB".into(),
            uri: "file:///run/ultar/media/u/USB".into(),
            area: AreaId::Drive("USB".into()),
        };
        let on = Loc::root(usb.area.clone()).child("x.apk").unwrap();
        assert_eq!(
            host_path(&on, &[usb]).as_deref(),
            Some("/run/ultar/media/u/USB/x.apk")
        );
        assert_eq!(host_path(&on, &[]), None);
        let trash = Loc::root(AreaId::Trash).child("a.deb").unwrap();
        assert_eq!(host_path(&trash, &[]), None);
    }

    #[test]
    fn permissions_start_off_whatever_the_file_suggests() {
        let mut s = Sheet::new("a.deb", "/p".into());
        s.inspected(Ok(found(Source::Deb, true)));
        assert!(matches!(s.phase, Phase::Ready(_)));
        assert_eq!(s.choices, Choices::default());
        s.set_lan(true);
        assert!(!s.choices.lan, "no local network without the internet");
        s.set_internet(true);
        s.set_lan(true);
        s.set_folder("downloads", true);
        s.set_folder("documents", true);
        s.set_folder("/etc", true);
        assert_eq!(s.choices.folders, ["documents", "downloads"]);
        s.set_internet(false);
        assert!(!s.choices.lan && s.choices.network == NetworkChoice::None);
    }

    #[test]
    fn a_job_runs_to_its_end() {
        let mut s = Sheet::new("a.deb", "/p".into());
        assert!(!s.start(), "nothing to install before the file was read");
        s.inspected(Ok(found(Source::Deb, true)));
        assert!(s.start());
        s.started(Ok(InstallOutcome::Started(4)));
        let p = |job, step| Progress {
            job,
            step,
            percent: 50,
            apps: vec!["foreign.deb-mousepad".into()],
        };
        assert!(!s.progress(&p(9, Step::Done)), "another job");
        assert!(s.progress(&p(4, Step::Install)));
        assert_eq!(s.job(), Some(4));
        assert!(s.progress(&p(4, Step::Done)));
        assert_eq!(s.phase, Phase::Done(vec!["foreign.deb-mousepad".into()]));
    }

    #[test]
    fn what_cannot_be_installed_says_why() {
        let cases = [
            (Ok(found(Source::Exe, false)), "files.foreign.not_yet"),
            (Ok(found(Source::Unknown, false)), "files.foreign.unknown"),
            (Ok(InspectOutcome::Unreadable), "files.foreign.unreadable"),
            (Ok(InspectOutcome::TooLarge), "files.foreign.too_large"),
            (Err(PortError::Unavailable), "files.foreign.no_service"),
        ];
        for (answer, key) in cases {
            let mut s = Sheet::new("a", "/p".into());
            s.inspected(answer);
            assert_eq!(s.phase, Phase::Problem(key));
        }
        let mut s = Sheet::new("a.deb", "/p".into());
        s.inspected(Ok(found(Source::Deb, true)));
        s.start();
        s.started(Ok(InstallOutcome::RuntimeMissing));
        assert_eq!(s.phase, Phase::Problem("files.foreign.runtime_missing"));
        let mut s = Sheet::new("a.deb", "/p".into());
        s.inspected(Ok(found(Source::Deb, true)));
        s.start();
        s.started(Ok(InstallOutcome::Started(1)));
        s.progress(&Progress {
            job: 1,
            step: Step::Failed(Failure::Refused),
            percent: 0,
            apps: Vec::new(),
        });
        assert_eq!(s.phase, Phase::Problem("files.foreign.failed.refused"));
    }
}
