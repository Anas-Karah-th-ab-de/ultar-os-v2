//! State of the page Settings › Apps from other systems (`docs/FREMD-APPS.md`).

use ultar_bus::messages::foreign::{ForeignApp, RuntimeStateReply};
use ultar_bus::Text;

/// The person's folders an app may be given, in the order shown (contract words).
pub const FOLDERS: [&str; 7] = [
    "documents",
    "downloads",
    "media-pictures",
    "media-music",
    "media-videos",
    "desktop",
    "home",
];

/// What the page knows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ForeignView {
    /// The Linux area (`None`: the service does not answer).
    pub runtime: Option<RuntimeStateReply>,
    /// The apps from other systems.
    pub apps: Vec<ForeignApp>,
    /// Why the service could not be read.
    pub error: Option<Text>,
    /// The app whose page is open.
    pub open: Option<String>,
    /// Keep the open app's data when it is removed.
    pub keep_data: bool,
    /// Jobs of this page that run (a wider grant waiting for its confirmation, the area's
    /// update): their end is said in a message.
    pub jobs: Vec<u64>,
}

impl ForeignView {
    /// The app whose page is open, while it is still installed.
    pub fn app(&self) -> Option<&ForeignApp> {
        let id = self.open.as_deref()?;
        self.apps.iter().find(|a| a.id == id)
    }
}
