//! The apps of the phone: what the home screen pages and the dock show.

use ultar_ui::icon::IconId;

/// An installed app as the shell shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Its identifier (`org.ultar.files`).
    pub id: String,
    /// Catalog key of its name (system apps) …
    pub key: Option<String>,
    /// … or its name as the package gives it (other apps).
    pub name: String,
    /// Its icon.
    pub icon: IconId,
}

impl App {
    /// A system app with its name from the shell's catalog (`mobile.app.<short>`).
    pub fn system(id: &str, short: &str, icon: IconId) -> App {
        App {
            id: id.to_owned(),
            key: Some(format!("mobile.app.{short}")),
            name: String::new(),
            icon,
        }
    }

    /// An app with the name its package gives.
    pub fn named(id: &str, name: &str, icon: IconId) -> App {
        App {
            id: id.to_owned(),
            key: None,
            name: name.to_owned(),
            icon,
        }
    }
}

/// Identifier of the phone app (its dock cell is the one in the accent colour).
pub const PHONE: &str = "org.ultar.phone";

/// The system apps that come with every phone, in the order of the home screen.
pub fn system_apps() -> Vec<App> {
    vec![
        App::system("org.ultar.settings", "settings", IconId::Gear),
        App::system("org.ultar.files", "files", IconId::Folder),
        App::system("org.ultar.store", "store", IconId::Bag),
        App::system("org.ultar.browser", "browser", IconId::Browser),
        App::system(PHONE, "phone", IconId::Phone),
        App::system("org.ultar.editor", "editor", IconId::Editor),
        App::system("org.ultar.office", "office", IconId::Office),
        App::system("org.ultar.terminal", "terminal", IconId::Terminal),
        App::system("org.ultar.assistant", "assistant", IconId::Spark),
        App::system("org.ultar.appbuilder", "appbuilder", IconId::Code),
    ]
}

/// The dock at the bottom of the home screen (old phone: camera, browser, messages, phone;
/// the phone last, at the end side).
pub fn default_dock() -> Vec<String> {
    [
        "org.ultar.files",
        "org.ultar.browser",
        "org.ultar.store",
        PHONE,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// Splits `apps` into pages of `per_page` (at least one page, even without apps).
pub fn pages(apps: &[App], per_page: usize) -> Vec<&[App]> {
    let per_page = per_page.max(1);
    if apps.is_empty() {
        return vec![&[]];
    }
    apps.chunks(per_page).collect()
}

/// Apps whose name (as shown) or identifier contains `query`, case and Arabic letter forms
/// ignored by the caller's `fold`.
pub fn matching<'a>(apps: &'a [App], query: &str, shown: impl Fn(&App) -> String) -> Vec<&'a App> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    apps.iter()
        .filter(|a| shown(a).to_lowercase().contains(&q) || a.id.to_lowercase().contains(&q))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_hold_at_most_per_page_and_never_none() {
        let apps = system_apps();
        let p = pages(&apps, 4);
        assert_eq!(p.len(), 3);
        assert_eq!(p[2].len(), 2);
        assert_eq!(pages(&[], 16).len(), 1);
        assert_eq!(pages(&apps, 0).len(), apps.len());
    }

    #[test]
    fn the_dock_ends_with_the_phone_and_every_dock_app_exists() {
        let dock = default_dock();
        assert_eq!(dock.last().map(String::as_str), Some(PHONE));
        let apps = system_apps();
        assert!(dock.iter().all(|d| apps.iter().any(|a| &a.id == d)));
    }

    #[test]
    fn search_finds_by_name_and_id() {
        let apps = system_apps();
        let hits = matching(&apps, "FIL", |a| a.id.clone());
        assert_eq!(hits.len(), 1);
        assert!(matching(&apps, "  ", |a| a.name.clone()).is_empty());
    }
}
