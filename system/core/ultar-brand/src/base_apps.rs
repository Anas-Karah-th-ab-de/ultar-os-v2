//! The base apps of Ultar OS: the apps the system is built on. The desktop, the Store and the
//! other apps rely on them, so the launcher never uninstalls them and the Store shows no
//! Remove for them; updates still replace them.

/// The base apps by launcher id (the shells name them `org.ultar.<id>`).
pub const BASE_APPS: [&str; 7] = [
    "settings", "files", "store", "phone", "browser", "editor", "terminal",
];

/// The prefix the shells put before the id of an app of the system.
const SHELL_PREFIX: &str = "org.ultar.";

/// Whether `app` (a launcher id like `files` or a shell id like `org.ultar.files`) is a base app.
pub fn is_base_app(app: &str) -> bool {
    let id = app.strip_prefix(SHELL_PREFIX).unwrap_or(app);
    BASE_APPS.contains(&id)
}

#[cfg(test)]
mod tests {
    use super::is_base_app;

    #[test]
    fn base_apps_are_known_by_both_names() {
        assert!(is_base_app("store") && is_base_app("org.ultar.settings"));
        assert!(is_base_app("terminal") && is_base_app("org.ultar.editor"));
        assert!(!is_base_app("km-code-craft.appbuilder"));
        assert!(!is_base_app("org.ultar.a.store") && !is_base_app("org.ultar."));
    }
}
