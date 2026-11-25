//! The services behind Files, as ports: the app launcher (open a file with its app) and the
//! drive service (mounted drives, eject). [`bus`] is the only place with bus calls; tests
//! use [`Fake`].

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
        }
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
