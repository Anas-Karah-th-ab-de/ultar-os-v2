//! The app's model: what the developer chose, what happened, what to show. Pure and
//! synchronous; the window runs [`run`] on a worker thread (nothing blocks the UI).

use crate::devkey::{self, DevKey};
use crate::error::{AppError, Issue};
use crate::package;
use crate::project::{self, Checked};
use std::path::{Path, PathBuf};

/// Something the developer can operate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// The project folder field.
    Project,
    /// "Check".
    Check,
    /// "Build package".
    Build,
    /// "Install for testing".
    Install,
}

/// Work for the worker thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Check the project folder.
    Check(PathBuf),
    /// Check, then pack and sign.
    Build(PathBuf),
    /// Install a built package.
    Install(PathBuf),
}

/// The worker's answer.
#[derive(Debug, Clone)]
pub enum Event {
    /// Result of [`Command::Check`].
    Checked(Result<Checked, Vec<Issue>>),
    /// Result of [`Command::Build`]: the package and the key that signed it.
    Built(Result<(PathBuf, DevKey), AppError>),
    /// Result of [`Command::Install`]: the installed app id.
    Installed(Result<String, AppError>),
}

/// What the status line says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Nothing done yet.
    Idle,
    /// Work is running.
    Busy,
    /// The project passed the check.
    Checked {
        /// App id.
        id: String,
        /// Version.
        version: String,
    },
    /// A package was built.
    Built(PathBuf),
    /// The package was installed.
    Installed(String),
    /// The last step failed.
    Failed(AppError),
}

/// The whole state of the window.
#[derive(Debug, Clone)]
pub struct State {
    /// Project folder as typed.
    pub project: String,
    /// Status.
    pub phase: Phase,
    /// Findings of the last check.
    pub issues: Vec<Issue>,
    /// The last built package.
    pub package: Option<PathBuf>,
    /// The developer key, once known.
    pub key: Option<DevKey>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            project: String::new(),
            phase: Phase::Idle,
            issues: Vec::new(),
            package: None,
            key: None,
        }
    }
}

impl State {
    /// True while the worker runs (buttons are disabled then).
    pub fn busy(&self) -> bool {
        self.phase == Phase::Busy
    }

    /// The project folder text changed.
    pub fn set_project(&mut self, text: &str) {
        if self.project != text {
            self.project = text.trim().to_string();
            self.package = None;
            self.issues.clear();
            self.phase = Phase::Idle;
        }
    }

    /// A button was pressed; answers the work to start, if any.
    pub fn act(&mut self, target: Target) -> Option<Command> {
        if self.busy() {
            return None;
        }
        let dir = PathBuf::from(&self.project);
        let cmd = match target {
            Target::Project => return None,
            Target::Check if !self.project.is_empty() => Command::Check(dir),
            Target::Build if !self.project.is_empty() => Command::Build(dir),
            Target::Install => match &self.package {
                Some(p) => Command::Install(p.clone()),
                None => {
                    self.phase = Phase::Failed(AppError::NotChecked);
                    return None;
                }
            },
            _ => {
                self.phase = Phase::Failed(AppError::Issues(vec![Issue::NoManifest]));
                return None;
            }
        };
        self.phase = Phase::Busy;
        Some(cmd)
    }

    /// Feeds the worker's answer back.
    pub fn event(&mut self, ev: Event) {
        self.phase = match ev {
            Event::Checked(Ok(c)) => {
                self.issues.clear();
                Phase::Checked {
                    id: c.id,
                    version: c.version,
                }
            }
            Event::Checked(Err(issues)) => {
                self.issues = issues.clone();
                Phase::Failed(AppError::Issues(issues))
            }
            Event::Built(Ok((path, key))) => {
                self.issues.clear();
                self.key = Some(key);
                self.package = Some(path.clone());
                Phase::Built(path)
            }
            Event::Built(Err(e)) => {
                if let AppError::Issues(list) = &e {
                    self.issues = list.clone();
                }
                Phase::Failed(e)
            }
            Event::Installed(Ok(id)) => Phase::Installed(id),
            Event::Installed(Err(e)) => Phase::Failed(e),
        };
    }
}

/// Does the work of `cmd`. `data_dir` holds the developer key; `install` hands a package to
/// the launcher (the bus in the app, a double in tests).
pub fn run(
    cmd: Command,
    data_dir: &Path,
    install: &dyn Fn(&Path) -> Result<String, AppError>,
) -> Event {
    match cmd {
        Command::Check(dir) => Event::Checked(project::check(&dir)),
        Command::Build(dir) => Event::Built((|| {
            let checked = project::check(&dir).map_err(AppError::Issues)?;
            let key = devkey::load_or_create(data_dir)?;
            let path = package::build(&checked, &key)?;
            Ok((path, key))
        })()),
        Command::Install(path) => Event::Installed(install(&path)),
    }
}
