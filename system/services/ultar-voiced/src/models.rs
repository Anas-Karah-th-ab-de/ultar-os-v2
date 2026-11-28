//! The speech-to-text models the person can choose from, and which of them are on the device.
//!
//! The models are the multilingual whisper models in GGML format (`ggml-<name>.bin`, MIT
//! licence, Arabic, English and German among about a hundred languages). They are data files,
//! not part of the `whisper` package: the device is offline, so they arrive with the offline
//! update (USB stick) into the models directory.

use std::path::{Path, PathBuf};
use ultar_bus::messages::voice::ModelInfo;

/// The known models: name and approximate size on disk in megabytes.
pub const KNOWN: [(&str, u32); 4] = [
    ("tiny", 75),
    ("base", 142),
    ("small", 466),
    ("medium", 1_533),
];

/// The model used when the person chose none: the best of the installed ones up to `small`
/// (`medium` needs more memory than a phone has; the person may still choose it).
pub const PREFERRED: [&str; 3] = ["small", "base", "tiny"];

/// The models directory.
#[derive(Clone, Debug)]
pub struct ModelCatalog {
    dir: PathBuf,
}

impl ModelCatalog {
    /// The catalog of `dir`.
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    /// The file of a model (`None` for a name that is not a known model).
    pub fn file(&self, name: &str) -> Option<PathBuf> {
        KNOWN
            .iter()
            .any(|(known, _)| *known == name)
            .then(|| self.dir.join(format!("ggml-{name}.bin")))
    }

    /// Whether the model's file is on the device.
    pub fn installed(&self, name: &str) -> bool {
        self.file(name).is_some_and(|f| f.is_file())
    }

    /// Every known model with its size and whether it is installed.
    pub fn list(&self) -> Vec<ModelInfo> {
        KNOWN
            .iter()
            .map(|(name, size)| ModelInfo {
                name: (*name).to_owned(),
                size_mb: *size,
                installed: self.installed(name),
            })
            .collect()
    }

    /// The model to use: the chosen one if it is installed, else the best installed
    /// preferred one (`None`: no model on the device).
    pub fn resolve(&self, chosen: &str) -> Option<String> {
        if self.installed(chosen) {
            return Some(chosen.to_owned());
        }
        PREFERRED
            .iter()
            .find(|name| self.installed(name))
            .map(|name| (*name).to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("voiced-models-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn only_known_models_with_a_file_count_as_installed() {
        let d = dir("list");
        let catalog = ModelCatalog::new(&d);
        assert!(catalog.list().iter().all(|m| !m.installed));
        assert_eq!(catalog.resolve(""), None);
        std::fs::write(d.join("ggml-base.bin"), b"x").unwrap();
        std::fs::write(d.join("ggml-evil.bin"), b"x").unwrap();
        assert!(catalog.installed("base"));
        assert!(!catalog.installed("evil"), "unknown names are never files");
        assert!(catalog.file("../base").is_none());
        let list = catalog.list();
        assert_eq!(list.len(), 4);
        assert_eq!(list.iter().filter(|m| m.installed).count(), 1);
        assert_eq!(list[2].size_mb, 466);
        // the choice wins when installed, else the best preferred one
        assert_eq!(catalog.resolve("small").as_deref(), Some("base"));
        std::fs::write(d.join("ggml-small.bin"), b"x").unwrap();
        assert_eq!(catalog.resolve("base").as_deref(), Some("base"));
        assert_eq!(catalog.resolve("").as_deref(), Some("small"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
