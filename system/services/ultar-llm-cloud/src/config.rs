//! What the person chose: provider, address, model. One small file in the service's
//! folder; it holds no secret.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The choice.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Config {
    /// Id of the provider (empty: the first one).
    #[serde(default)]
    pub provider: String,
    /// Address, for a provider that needs one.
    #[serde(default)]
    pub endpoint: String,
    /// Model id (empty: a small one of the provider).
    #[serde(default)]
    pub model: String,
}

/// Where the choice is kept.
#[derive(Debug, Clone)]
pub struct ConfigFile(PathBuf);

impl ConfigFile {
    /// The file in `dir`.
    pub fn new(dir: &Path) -> Self {
        Self(dir.join("config.json"))
    }

    /// The stored choice; the default when none (or an unreadable one) is stored.
    pub fn read(&self) -> Config {
        let text = std::fs::read_to_string(&self.0).unwrap_or_default();
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Stores the choice.
    pub fn write(&self, config: &Config) -> bool {
        let part = self.0.with_extension("part");
        let text = serde_json::to_string(config).unwrap_or_default();
        std::fs::write(&part, text).is_ok() && std::fs::rename(&part, &self.0).is_ok()
    }
}
