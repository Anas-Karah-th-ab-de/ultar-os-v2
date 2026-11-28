//! The desktop background (DESKTOP-SPEC §5.1 "Arbeitsfläche"): its right-click menu (new
//! shortcut, icon size, show icons, sort, background, display and desktop settings, the
//! terminal) and the app shortcuts on it – a click selects one, a double click opens it,
//! its right-click menu opens, renames, pins, removes it or shows its properties. A
//! shortcut is added from the background's menu ("New shortcut" opens the app grid to
//! choose the app) or from an app's menu in the app grid.

use super::bar::{launch, SETTINGS};
use super::{Effect, Opening, Outcome, Shell};
use crate::model::places;
use crate::model::prefs::IconSize;
use crate::model::Open;
use crate::view::Target;
use ultar_bus::messages::settings::SettingValue;
use ultar_ui::event::ActionKind;
use ultar_ui::icon::IconId;
use ultar_ui::widgets::{Label, Menu, MenuItem};

/// The terminal ("Open in Terminal").
pub const TERMINAL: &str = "org.ultar.terminal";

/// Items of the background's menu.
pub mod item {
    /// New shortcut (choose the app in the app grid).
    pub const NEW_SHORTCUT: u32 = 1;
    /// New folder (the desktop folder is Files'; not yet reachable from the shell).
    pub const NEW_FOLDER: u32 = 2;
    /// Paste (as above).
    pub const PASTE: u32 = 3;
    /// Icon sizes: large, medium, small (`SIZE + index of IconSize::ALL`).
    pub const SIZE: u32 = 10;
    /// Show the desktop icons.
    pub const SHOW: u32 = 13;
    /// Sort by name.
    pub const SORT: u32 = 14;
    /// Change the background (Settings › Appearance).
    pub const WALLPAPER: u32 = 20;
    /// Display settings.
    pub const DISPLAY: u32 = 21;
    /// Desktop settings (Settings › Desktop bar, where the icons are set).
    pub const SETTINGS: u32 = 22;
    /// Open in Terminal.
    pub const TERMINAL: u32 = 23;
}

/// Items of a shortcut's menu.
pub mod shortcut {
    /// Open.
    pub const OPEN: u32 = 1;
    /// Rename.
    pub const RENAME: u32 = 2;
    /// Pin to the bar / unpin.
    pub const PIN: u32 = 3;
    /// Remove from the desktop.
    pub const REMOVE: u32 = 4;
    /// Properties (the app's page in Settings).
    pub const PROPERTIES: u32 = 5;
}

/// Keys of the sizes in the menu, large first (Windows' order).
const SIZES: [(IconSize, &str); 3] = [
    (IconSize::Large, "desk.desk.size.large"),
    (IconSize::Medium, "desk.desk.size.medium"),
    (IconSize::Small, "desk.desk.size.small"),
];

/// The background's menu.
pub(super) fn desk_menu(s: &Shell) -> Menu {
    let gate = |i: MenuItem| {
        if s.d.presence.launcher {
            i
        } else {
            i.disabled()
        }
    };
    let desk = s.d.prefs.desk;
    let mut items = vec![
        MenuItem::new(item::NEW_SHORTCUT, Label::key("desk.desk.new_shortcut")).icon(IconId::Plus),
        MenuItem::new(item::NEW_FOLDER, Label::key("desk.desk.new_folder")).disabled(),
        MenuItem::new(item::PASTE, Label::key("desk.desk.paste")).disabled(),
    ];
    for (i, (size, key)) in SIZES.iter().enumerate() {
        let entry =
            MenuItem::new(item::SIZE + i as u32, Label::key(*key)).checked(desk.size == *size);
        items.push(if i == 0 { entry.separated() } else { entry });
    }
    items.push(MenuItem::new(item::SHOW, Label::key("desk.desk.show")).checked(desk.shown));
    let sort = MenuItem::new(item::SORT, Label::key("desk.desk.sort"));
    items.push(if s.d.places.shortcuts.len() > 1 && desk.shown {
        sort
    } else {
        sort.disabled()
    });
    items.extend([
        gate(MenuItem::new(item::WALLPAPER, Label::key("desk.desk.wallpaper")).separated()),
        gate(MenuItem::new(
            item::DISPLAY,
            Label::key("desk.desk.display"),
        )),
        gate(MenuItem::new(
            item::SETTINGS,
            Label::key("desk.desk.settings"),
        )),
    ]);
    let terminal = MenuItem::new(item::TERMINAL, Label::key("desk.desk.terminal")).separated();
    let installed = s.d.catalog.iter().any(|a| a.id == TERMINAL);
    items.push(if installed {
        gate(terminal)
    } else {
        terminal.disabled()
    });
    Menu::new(items)
}

/// The menu of shortcut `index`.
pub(super) fn shortcut_menu(s: &Shell, index: usize) -> Menu {
    let Some(app) = s.d.places.shortcuts.get(index).map(|c| c.app.as_str()) else {
        return Menu::new(Vec::new());
    };
    let gate = |i: MenuItem| {
        if s.d.presence.launcher {
            i
        } else {
            i.disabled()
        }
    };
    let pin = if s.d.places.pinned.iter().any(|p| p == app) {
        "desk.menu.unpin"
    } else {
        "desk.menu.pin"
    };
    Menu::new(vec![
        gate(MenuItem::new(shortcut::OPEN, Label::key("desk.menu.open"))),
        MenuItem::new(shortcut::RENAME, Label::key("desk.desk.rename")).separated(),
        MenuItem::new(shortcut::PIN, Label::key(pin)),
        MenuItem::new(shortcut::REMOVE, Label::key("desk.desk.remove_shortcut")).danger(),
        gate(MenuItem::new(shortcut::PROPERTIES, Label::key("desk.desk.properties")).separated()),
    ])
}

/// The secondary button on the background at screen point `at`: on a shortcut its menu
/// (and it is selected), elsewhere the background's menu.
pub(super) fn open_at(s: &mut Shell, target: Option<&Target>, at: (i32, i32)) -> Outcome {
    s.d.open = match target {
        Some(Target::Shortcut(i)) if *i < s.d.places.shortcuts.len() => {
            s.d.places.selected = Some(*i);
            Open::ShortcutMenu(*i, at.0, at.1)
        }
        _ => {
            s.d.places.selected = None;
            Open::DeskMenu(at.0, at.1)
        }
    };
    Outcome::surfaces()
}

/// A click on the background: a shortcut is selected; a double click opens it.
pub(super) fn on_desk(s: &mut Shell, target: &Target, kind: &ActionKind) -> Outcome {
    let Target::Shortcut(i) = target else {
        return Outcome::none();
    };
    match kind {
        ActionKind::DoubleActivated => open(s, *i),
        ActionKind::Activated if s.d.places.selected != Some(*i) => {
            s.d.places.selected = Some(*i);
            background()
        }
        _ => Outcome::none(),
    }
}

/// Only the background changed (it is built again with the bar).
fn background() -> Outcome {
    Outcome {
        effects: Vec::new(),
        bar: true,
        popup: false,
    }
}

/// The shortcuts changed: the background is built again and the list is written down.
fn changed() -> Outcome {
    Outcome {
        effects: vec![Effect::Shortcuts],
        ..Outcome::surfaces()
    }
}

/// Opens shortcut `index`: the app's window comes forward, or it starts.
fn open(s: &mut Shell, index: usize) -> Outcome {
    match s.d.places.shortcuts.get(index).map(|c| c.app.clone()) {
        Some(app) => super::sheets::open_app(s, &app),
        None => Outcome::none(),
    }
}

/// A chosen item of the background's menu.
pub(super) fn desk_chosen(s: &mut Shell, id: u32) -> Outcome {
    match id {
        item::NEW_SHORTCUT => {
            s.d.overview.query.clear();
            s.d.open = Open::Apps(None);
            s.d.places.picking = true;
            Outcome::surfaces()
        }
        i if (item::SIZE..item::SIZE + 3).contains(&i) => {
            let size = SIZES[(i - item::SIZE) as usize].0;
            s.d.prefs.desk.size = size;
            setting("desktop.icons.size", SettingValue::Text(size.word().into()))
        }
        item::SHOW => {
            let shown = !s.d.prefs.desk.shown;
            s.d.prefs.desk.shown = shown;
            setting("desktop.icons.enabled", SettingValue::Flag(shown))
        }
        item::SORT => {
            let (catalog, tr) = (&s.d.catalog, &s.tr);
            s.d.places.sort(|c| places::shown_name(c, catalog, tr));
            changed()
        }
        item::WALLPAPER => launch(s, SETTINGS, Opening::settings_page("appearance")),
        item::DISPLAY => launch(s, SETTINGS, Opening::settings_page("display")),
        item::SETTINGS => launch(s, SETTINGS, Opening::settings_page("desktop_bar")),
        item::TERMINAL => launch(s, TERMINAL, Opening::start()),
        _ => Outcome::none(),
    }
}

/// A setting changed from the menu: shown at once, stored in the settings.
fn setting(key: &str, value: SettingValue) -> Outcome {
    Outcome {
        effects: vec![Effect::Setting(key.to_owned(), value)],
        ..Outcome::surfaces()
    }
}

/// A chosen item of shortcut `index`'s menu.
pub(super) fn shortcut_chosen(s: &mut Shell, index: usize, id: u32) -> Outcome {
    let Some(app) = s.d.places.shortcuts.get(index).map(|c| c.app.clone()) else {
        return Outcome::none();
    };
    match id {
        shortcut::OPEN => open(s, index),
        shortcut::RENAME => {
            s.d.places.draft =
                places::shown_name(&s.d.places.shortcuts[index], &s.d.catalog, &s.tr);
            s.d.open = Open::Rename(index);
            Outcome::surfaces()
        }
        shortcut::PIN => {
            let pinned = &mut s.d.places.pinned;
            if pinned.contains(&app) {
                pinned.retain(|p| *p != app);
            } else {
                pinned.push(app);
            }
            Outcome::surfaces()
        }
        shortcut::REMOVE => {
            s.d.places.remove(index);
            changed()
        }
        shortcut::PROPERTIES => launch(s, SETTINGS, Opening::settings_page(&format!("apps/{app}"))),
        _ => Outcome::none(),
    }
}

/// An app chosen in the app grid opened by "New shortcut": its shortcut goes on the desktop.
pub(super) fn picked(s: &mut Shell, app: &str) -> Outcome {
    s.d.places.picking = false;
    s.d.open = Open::None;
    s.d.overview.query.clear();
    s.d.places.add(app);
    // A hidden desktop shows its icons again: the new shortcut is meant to be seen.
    let mut out = changed();
    if !s.d.prefs.desk.shown {
        s.d.prefs.desk.shown = true;
        out.effects.push(Effect::Setting(
            "desktop.icons.enabled".into(),
            SettingValue::Flag(true),
        ));
    }
    out
}

/// "Add to desktop" / "Remove from desktop" in an app's menu of the app grid.
pub(super) fn toggle_shortcut(s: &mut Shell, app: &str) -> Outcome {
    if let Some(i) = s.d.places.shortcuts.iter().position(|c| c.app == app) {
        s.d.places.remove(i);
        return changed();
    }
    if !s.d.catalog.iter().any(|a| a.id == app) {
        return Outcome::none();
    }
    s.d.places.add(app);
    changed()
}

/// Text typed into the rename dialog.
pub(super) fn typed(s: &mut Shell, text: &str) -> Outcome {
    s.d.places.draft = text.chars().take(places::MAX_NAME).collect();
    Outcome::none()
}

/// The rename dialog's "Rename" (or Enter): the typed name is the shortcut's.
pub(super) fn rename_done(s: &mut Shell) -> Outcome {
    let Open::Rename(index) = s.d.open else {
        return Outcome::none();
    };
    let app_name =
        s.d.places
            .shortcuts
            .get(index)
            .and_then(|c| s.d.catalog.iter().find(|a| a.id == c.app))
            .map(|a| a.display_name(&s.tr))
            .unwrap_or_default();
    let draft = std::mem::take(&mut s.d.places.draft);
    s.d.places.rename(index, &draft, &app_name);
    s.d.open = Open::None;
    changed()
}
