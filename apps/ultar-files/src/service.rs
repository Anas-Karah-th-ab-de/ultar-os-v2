//! The services behind Files, as ports: the app launcher (open a file with its app), the
//! drive service (mounted drives, eject) and the service for apps from other systems
//! (install a `.deb`, `.exe`, `.msi` or `.apk`). [`bus`] is the only place with bus calls;
//! tests use [`Fake`].

pub mod bus;

use std::collections::VecDeque;

/// Why a service call did not do what was asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortError {
    /// The service is not running or Files may not call it.
    Unavailable,
    /// The app to open with is not installed.
    NotInstalled,
    /// The service refused.
    Refused,
}

impl PortError {
    /// i18n key of the message.
    pub fn key(self) -> &'static str {
        match self {
            PortError::Unavailable => "files.error.service",
            PortError::NotInstalled => "files.notice.no_app",
            PortError::Refused => "files.error.refused",
        }
    }
}

impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key())
    }
}

impl std::error::Error for PortError {}

/// One mounted drive as the drive service reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mounted {
    /// Id for eject.
    pub id: String,
    /// Label.
    pub name: String,
    /// `file://` URI of its folder.
    pub uri: String,
}

/// The app launcher (`launcher.resolve` and `launcher.start` version 2).
pub trait LauncherPort {
    /// The default app for `uri`: the launcher resolves it through `apps.default.*` and
    /// keeps only an installed app that declares the URI. `None`: no app opens it.
    fn resolve(&mut self, uri: &str) -> Result<Option<String>, PortError>;
    /// Starts `app` with `uri` to open (`activation_token` from the compositor, if any).
    fn open(
        &mut self,
        app: &str,
        uri: &str,
        activation_token: Option<u64>,
    ) -> Result<(), PortError>;
    /// Starts `app` without anything to open (an app just installed).
    fn start(&mut self, app: &str) -> Result<(), PortError>;
}

/// What the service for apps from other systems answered (it works in the background:
/// reading a file and installing take longer than a frame).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForeignEvent {
    /// The answer to [`ForeignPort::inspect`].
    Inspected(Result<ultar_bus::messages::foreign::InspectOutcome, PortError>),
    /// The answer to [`ForeignPort::install`].
    Started(Result<ultar_bus::messages::foreign::InstallOutcome, PortError>),
    /// A step of a job (`foreign.progress`).
    Progress(ultar_bus::messages::foreign::Progress),
}

/// The service for apps from other systems (`foreign`): asks only, never waits.
pub trait ForeignPort {
    /// Asks what the file at `path` (as the device sees it) is.
    fn inspect(&mut self, path: &str);
    /// Asks to install the file at `path` with what the person granted.
    fn install(&mut self, path: &str, choices: ultar_bus::messages::foreign::Choices);
    /// The answers and job steps that arrived since the last call.
    fn poll(&mut self) -> Vec<ForeignEvent>;
}

/// The drive service (`volumes`).
pub trait VolumesPort {
    /// Ejects a drive: `Ok(true)` done, `Ok(false)` busy (files are open on it).
    fn eject(&mut self, id: &str) -> Result<bool, PortError>;
    /// The mounted drives when they changed since the last call.
    fn changed(&mut self) -> Option<Vec<Mounted>>;
}

/// Ports of a test or of a window without services: records every call.
#[derive(Debug)]
pub struct Fake {
    /// `(app, uri)` of every open.
    pub opened: Vec<(String, String)>,
    /// Apps that are installed (others answer `NotInstalled`); empty = all.
    pub installed: Vec<String>,
    /// Ids ejected.
    pub ejected: Vec<String>,
    /// Ids that answer busy.
    pub busy: Vec<String>,
    /// Drive states to report, oldest first.
    pub drive_states: VecDeque<Vec<Mounted>>,
    /// The default app of each kind (as `apps.default.*` names them), which `resolve`
    /// follows; a test changes these to see the opened app change.
    pub defaults: Vec<(ultar_bus::messages::launcher::Kind, String)>,
    /// Paths given to `foreign.inspect`.
    pub inspected: Vec<String>,
    /// `(path, choices)` of every `foreign.install`.
    pub installs: Vec<(String, ultar_bus::messages::foreign::Choices)>,
    /// What `foreign.inspect` answers (`None`: the service is not there).
    pub inspect_answer: Option<ultar_bus::messages::foreign::InspectOutcome>,
    /// What `foreign.install` answers.
    pub install_answer: ultar_bus::messages::foreign::InstallOutcome,
    /// Events still to deliver, oldest first (a test adds job steps here).
    pub foreign_events: VecDeque<ForeignEvent>,
}

impl Default for Fake {
    fn default() -> Self {
        use ultar_bus::messages::launcher::Kind;
        let app = |k: Kind, a: &str| (k, a.to_owned());
        Self {
            opened: Vec::new(),
            installed: Vec::new(),
            ejected: Vec::new(),
            busy: Vec::new(),
            drive_states: VecDeque::new(),
            defaults: vec![
                app(Kind::Web, "org.ultar.web"),
                app(Kind::Mail, "org.ultar.mail"),
                app(Kind::Calendar, "org.ultar.calendar"),
                app(Kind::Music, "org.ultar.media"),
                app(Kind::Video, "org.ultar.media"),
                app(Kind::Photos, "org.ultar.images"),
                app(Kind::Documents, "org.ultar.office"),
            ],
            inspected: Vec::new(),
            installs: Vec::new(),
            inspect_answer: None,
            install_answer: ultar_bus::messages::foreign::InstallOutcome::Started(1),
            foreign_events: VecDeque::new(),
        }
    }
}

impl ForeignPort for Fake {
    fn inspect(&mut self, path: &str) {
        self.inspected.push(path.to_owned());
        let answer = self.inspect_answer.clone().ok_or(PortError::Unavailable);
        self.foreign_events
            .push_back(ForeignEvent::Inspected(answer));
    }

    fn install(&mut self, path: &str, choices: ultar_bus::messages::foreign::Choices) {
        self.installs.push((path.to_owned(), choices));
        self.foreign_events
            .push_back(ForeignEvent::Started(Ok(self.install_answer)));
    }

    fn poll(&mut self) -> Vec<ForeignEvent> {
        self.foreign_events.drain(..).collect()
    }
}

impl LauncherPort for Fake {
    fn resolve(&mut self, uri: &str) -> Result<Option<String>, PortError> {
        let Some(kind) = ultar_bus::messages::launcher::kind_of_uri(uri) else {
            return Ok(None);
        };
        Ok(self
            .defaults
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, a)| a.clone()))
    }

    fn open(&mut self, app: &str, uri: &str, _token: Option<u64>) -> Result<(), PortError> {
        if !self.installed.is_empty() && !self.installed.iter().any(|a| a == app) {
            return Err(PortError::NotInstalled);
        }
        self.opened.push((app.to_owned(), uri.to_owned()));
        Ok(())
    }

    fn start(&mut self, app: &str) -> Result<(), PortError> {
        self.open(app, "", None)
    }
}

impl VolumesPort for Fake {
    fn eject(&mut self, id: &str) -> Result<bool, PortError> {
        if self.busy.iter().any(|b| b == id) {
            return Ok(false);
        }
        self.ejected.push(id.to_owned());
        Ok(true)
    }

    fn changed(&mut self) -> Option<Vec<Mounted>> {
        self.drive_states.pop_front()
    }
}
