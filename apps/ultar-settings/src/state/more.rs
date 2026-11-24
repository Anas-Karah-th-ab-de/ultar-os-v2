//! State of the About, Users, Apps and Storage pages: the installed release and what their
//! services reported.

use crate::service::system::{AppEntry, Drive, LocalUser};
use ultar_bus::Text;

/// What the three services reported, or why one could not be read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MoreState {
    /// Version and build of the installed release (About), `None` when unknown.
    pub release: Option<(String, String)>,
    /// Local users of the account service.
    pub users: Vec<LocalUser>,
    /// Why the users could not be read.
    pub users_error: Option<Text>,
    /// Installed apps with their permissions (launcher).
    pub apps: Vec<AppEntry>,
    /// Why the apps could not be read.
    pub apps_error: Option<Text>,
    /// Mounted drives (volume service).
    pub drives: Vec<Drive>,
    /// Why the drives could not be read.
    pub drives_error: Option<Text>,
}
