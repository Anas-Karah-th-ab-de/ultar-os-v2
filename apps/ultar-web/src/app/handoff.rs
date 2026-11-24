//! Handing a downloaded file to another app ("Show in folder" opens the Files app with the
//! file selected). The Files app is started as `<program> --select <file>`; when it is not
//! installed the user is told so – nothing pretends to have worked.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Why a file could not be handed over.
#[derive(Debug)]
pub enum HandoffError {
    /// The app is not installed.
    Missing,
    /// Starting it failed.
    Start(std::io::Error),
}

impl fmt::Display for HandoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandoffError::Missing => f.write_str("files app not installed"),
            HandoffError::Start(e) => write!(f, "files app could not start: {e}"),
        }
    }
}

impl std::error::Error for HandoffError {}

/// Shows files in another app.
pub trait Handoff {
    /// Shows `file` in its folder.
    fn show_in_folder(&mut self, file: &Path) -> Result<(), HandoffError>;
}

/// The Files app as a program.
#[derive(Debug, Clone)]
pub struct FilesApp {
    /// Its program.
    pub program: PathBuf,
}

impl Handoff for FilesApp {
    fn show_in_folder(&mut self, file: &Path) -> Result<(), HandoffError> {
        if !self.program.is_file() {
            return Err(HandoffError::Missing);
        }
        Command::new(&self.program)
            .arg("--select")
            .arg(file)
            .stdin(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(HandoffError::Start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_files_app_is_reported() {
        let mut app = FilesApp {
            program: PathBuf::from("/nonexistent/ultar-files"),
        };
        assert!(matches!(
            app.show_in_folder(Path::new("/x")),
            Err(HandoffError::Missing)
        ));
        assert!(!HandoffError::Missing.to_string().is_empty());
    }
}
