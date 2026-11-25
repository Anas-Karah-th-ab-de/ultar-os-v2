//! Apps: the catalog (name, icon), the pinned ones of the bar and the windows that run,
//! grouped per app the way the bar shows them (pinned first, then running ones).

use ultar_i18n::{collation, Translator};
use ultar_surface::proto::ShellWindow;
use ultar_ui::icon::IconId;
use ultar_ui::widgets::Label;

/// Most apps the catalog holds.
pub const MAX_APPS: usize = 256;
/// Most tiles of the bar (the rest is reached through "all apps").
pub const MAX_BAR_TILES: usize = 16;

/// How an app is named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppName {
    /// A catalog key of the base system (`desk.app.files`).
    Key(String),
    /// The name from the app's manifest (data, shown as it is).
    Data(String),
}

/// Categories of the app grid (CLICKS `apps.category`), in the order of its tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    /// Browsers, mail.
    Internet,
    /// Phone, messages.
    Communication,
    /// Files, editors, terminal, the assistant.
    Tools,
    /// Settings, store.
    System,
}

impl Category {
    /// Every category, in tab order.
    pub const ALL: [Category; 4] = [
        Category::Internet,
        Category::Communication,
        Category::Tools,
        Category::System,
    ];

    /// Catalog key of its name.
    pub const fn key(self) -> &'static str {
        match self {
            Category::Internet => "desk.apps.cat.internet",
            Category::Communication => "desk.apps.cat.communication",
            Category::Tools => "desk.apps.cat.tools",
            Category::System => "desk.apps.cat.system",
        }
    }
}

/// One app of the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    /// Application id (`org.ultar.files`).
    pub id: String,
    /// Its name.
    pub name: AppName,
    /// Its icon.
    pub icon: IconId,
    /// Its category (none: listed under "All" only).
    pub category: Option<Category>,
    /// Part of the base system (cannot be removed).
    pub base: bool,
}

impl AppInfo {
    /// The name as a label.
    pub fn label(&self) -> Label {
        match &self.name {
            AppName::Key(k) => Label::key(k.clone()),
            AppName::Data(d) => Label::data(d),
        }
    }

    /// The name in the current language.
    pub fn display_name(&self, tr: &Translator) -> String {
        match &self.name {
            AppName::Key(k) => tr.t(k).into_owned(),
            AppName::Data(d) => d.clone(),
        }
    }
}

fn base(id: &str, key: &str, icon: IconId, category: Category) -> AppInfo {
    AppInfo {
        id: id.to_owned(),
        name: AppName::Key(key.to_owned()),
        icon,
        category: Some(category),
        base: true,
    }
}

/// Apps of the base system that are not pinned by default.
fn more_base_apps() -> Vec<AppInfo> {
    vec![
        base(
            "org.ultar.assistant",
            "desk.app.assistant",
            IconId::Spark,
            Category::Tools,
        ),
        base(
            "org.ultar.phone",
            "desk.app.phone",
            IconId::Phone,
            Category::Communication,
        ),
    ]
}

/// An installed app the launcher reports (`launcher.list`): a base app keeps its name,
/// icon and category; any other is named by the last part of its id (the launcher's list
/// carries no manifest name, icon or category yet).
pub fn installed(id: &str) -> AppInfo {
    if let Some(known) = base_apps()
        .into_iter()
        .chain(more_base_apps())
        .find(|a| a.id == id)
    {
        return known;
    }
    let short = id.rsplit('.').next().unwrap_or(id);
    let mut name: String = short.chars().take(1).flat_map(char::to_uppercase).collect();
    name.extend(short.chars().skip(1));
    AppInfo {
        id: id.to_owned(),
        name: AppName::Data(name),
        icon: IconId::Apps,
        category: None,
        base: false,
    }
}

/// The apps of the base system the boards show (DESKTOP-SPEC §7.1, §9) with their icons
/// (ICONS.md). The launcher's catalog replaces this list when it is available.
pub fn base_apps() -> Vec<AppInfo> {
    vec![
        base(
            "org.ultar.files",
            "desk.app.files",
            IconId::Folder,
            Category::Tools,
        ),
        base(
            "org.ultar.browser",
            "desk.app.browser",
            IconId::Browser,
            Category::Internet,
        ),
        base(
            "org.ultar.terminal",
            "desk.app.terminal",
            IconId::Terminal,
            Category::Tools,
        ),
        base(
            "org.ultar.editor",
            "desk.app.editor",
            IconId::Editor,
            Category::Tools,
        ),
        base(
            "org.ultar.store",
            "desk.app.store",
            IconId::Bag,
            Category::System,
        ),
        base(
            "org.ultar.settings",
            "desk.app.settings",
            IconId::Gear,
            Category::System,
        ),
    ]
}

/// The pinned apps of the standard bar (boards: Main, DesktopBottom).
pub fn default_pinned() -> Vec<String> {
    base_apps().into_iter().map(|a| a.id).collect()
}

/// One tile of the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarApp {
    /// The app (from the catalog, or made up from the window for unknown ids).
    pub app: AppInfo,
    /// Pinned.
    pub pinned: bool,
    /// Its windows, bottom to top.
    pub windows: Vec<ShellWindow>,
}

impl BarApp {
    /// One of its windows has the focus.
    pub fn active(&self) -> bool {
        self.windows.iter().any(|w| w.focused)
    }
}

fn unknown(window: &ShellWindow) -> AppInfo {
    let name = if window.title.is_empty() {
        window.app_id.clone()
    } else {
        window.title.clone()
    };
    AppInfo {
        id: window.app_id.clone(),
        name: AppName::Data(name),
        icon: IconId::Apps,
        category: None,
        base: false,
    }
}

/// The tiles: pinned apps in their order, then apps that run without being pinned, in the
/// order their first window appeared; at most [`MAX_BAR_TILES`].
pub fn bar_apps(pinned: &[String], catalog: &[AppInfo], windows: &[ShellWindow]) -> Vec<BarApp> {
    let mut out: Vec<BarApp> = pinned
        .iter()
        .filter_map(|id| catalog.iter().find(|a| &a.id == id))
        .map(|app| BarApp {
            app: app.clone(),
            pinned: true,
            windows: Vec::new(),
        })
        .collect();
    let mut order: Vec<u32> = windows.iter().map(|w| w.id).collect();
    order.sort_unstable();
    for id in order {
        let Some(w) = windows.iter().find(|w| w.id == id) else {
            continue;
        };
        match out.iter_mut().find(|b| b.app.id == w.app_id) {
            Some(tile) => tile.windows.push(w.clone()),
            None => {
                let app = catalog
                    .iter()
                    .find(|a| a.id == w.app_id)
                    .cloned()
                    .unwrap_or_else(|| unknown(w));
                out.push(BarApp {
                    app,
                    pinned: false,
                    windows: vec![w.clone()],
                });
            }
        }
    }
    out.truncate(MAX_BAR_TILES);
    out
}

/// An Arabic name without its article "ال" (the grid ignores it when sorting, §5.1).
pub fn without_article(name: &str) -> &str {
    // Alef + lam: the article is a rule of the language, not a text to translate.
    match name.strip_prefix("\u{0627}\u{0644}") {
        Some(rest) if rest.chars().count() > 1 => rest,
        _ => name,
    }
}

/// The catalog in alphabetical order of the language (Arabic without the article).
pub fn sorted(catalog: &[AppInfo], tr: &Translator) -> Vec<AppInfo> {
    let mut list: Vec<(collation::CollationKey, AppInfo)> = catalog
        .iter()
        .take(MAX_APPS)
        .map(|a| {
            (
                collation::collation_key(without_article(&a.display_name(tr))),
                a.clone(),
            )
        })
        .collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));
    list.into_iter().map(|(_, a)| a).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_surface::proto::{OutputId, WindowState};

    fn win(id: u32, app: &str, focused: bool) -> ShellWindow {
        ShellWindow {
            id,
            app_id: app.into(),
            title: format!("t{id}"),
            state: WindowState::Normal,
            focused,
            unresponsive: false,
            output: OutputId(0),
            workspace: 0,
        }
    }

    #[test]
    fn pinned_first_then_running_grouped_by_app() {
        let catalog = base_apps();
        let pinned = vec!["org.ultar.files".to_owned(), "org.ultar.missing".to_owned()];
        let windows = [
            win(3, "org.ultar.terminal", false),
            win(1, "org.ultar.files", false),
            win(2, "org.ultar.files", true),
            win(4, "com.example.x", false),
        ];
        let tiles = bar_apps(&pinned, &catalog, &windows);
        let ids: Vec<&str> = tiles.iter().map(|t| t.app.id.as_str()).collect();
        assert_eq!(
            ids,
            ["org.ultar.files", "org.ultar.terminal", "com.example.x"]
        );
        assert_eq!(tiles[0].windows.len(), 2);
        assert!(tiles[0].pinned && tiles[0].active());
        assert!(!tiles[1].pinned && !tiles[1].active());
        assert_eq!(tiles[2].app.name, AppName::Data("t4".into()));
        let many: Vec<ShellWindow> = (0..40).map(|i| win(i, &format!("a{i}"), false)).collect();
        assert_eq!(bar_apps(&[], &catalog, &many).len(), MAX_BAR_TILES);
    }

    #[test]
    fn apps_sort_in_the_order_of_the_language() {
        let tr = crate::i18n::translator(ultar_i18n::Lang::En).unwrap();
        let names: Vec<String> = sorted(&base_apps(), &tr)
            .iter()
            .map(|a| a.display_name(&tr))
            .collect();
        let mut expected = names.clone();
        expected.sort();
        assert_eq!(names, expected);
        let ar = crate::i18n::translator(ultar_i18n::Lang::Ar).unwrap();
        let first = sorted(&base_apps(), &ar)[0].display_name(&ar);
        assert!(first.starts_with('ا'), "{first}");
        assert_eq!(default_pinned().len(), 6);
    }
}
