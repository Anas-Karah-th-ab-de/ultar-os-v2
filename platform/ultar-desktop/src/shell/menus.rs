//! Menus of the bar: the input languages (shell.kbd), the power menu (qs.power), the menu
//! of an app tile (shell.app right click: its windows, new window, pin/unpin, quit) and of
//! the trash (open, empty – the files service does not exist yet).

use super::bar::{launch, FILES, SETTINGS, STORE, TRASH_URI};
use super::{Effect, Opening, Outcome, Shell};
use crate::model::services::Call;
use crate::model::Open;
use crate::view::Target;
use ultar_surface::proto::WindowAction;
use ultar_ui::icon::IconId;
use ultar_ui::widgets::{Label, Menu, MenuItem};

/// Which menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    /// Input languages.
    Keyboard,
    /// Power.
    Power,
    /// Audio outputs.
    Outputs,
    /// A drive tile.
    Drive(usize),
    /// An app tile.
    Tile(usize),
    /// The trash tile.
    Trash,
    /// The active app (top bar).
    App,
}

/// Item ids: layouts are 0..8, windows are `WINDOW_BASE + index`.
pub const MENU_KEYBOARD_SETTINGS: u32 = 100;
const NEW_WINDOW: u32 = 101;
const PIN: u32 = 102;
const QUIT: u32 = 103;
const OPEN: u32 = 104;
const EMPTY_TRASH: u32 = 105;
const ABOUT: u32 = 106;
const APP_SETTINGS: u32 = 107;
const EJECT: u32 = 108;
const HANDOFF: u32 = 109;

/// The devices to continue on start here (index into `Services::targets`).
const TARGET_BASE: u32 = 500;
const POWER_BASE: u32 = 200;
const WINDOW_BASE: u32 = 300;

/// Audio output entries start here (index into `Services::outputs`).
const OUTPUT_BASE: u32 = 400;

/// Power menu entries: suspend, restart, power off (`power.actions`), sign out (session
/// service), switch user (the lock screen offers the accounts).
const POWER: [&str; 5] = [
    "desk.power.suspend",
    "desk.power.restart",
    "desk.power.off",
    "desk.power.sign_out",
    "desk.power.switch_user",
];

/// The menu that is open, if any.
pub fn open_menu(s: &Shell) -> Option<(MenuKind, Menu)> {
    let kind = match s.d.open {
        Open::KeyboardMenu => MenuKind::Keyboard,
        Open::PowerMenu => MenuKind::Power,
        Open::OutputMenu => MenuKind::Outputs,
        Open::DriveMenu(i) => MenuKind::Drive(i),
        Open::TileMenu(usize::MAX) => MenuKind::Trash,
        Open::TileMenu(i) => MenuKind::Tile(i),
        Open::AppMenu => MenuKind::App,
        _ => return None,
    };
    Some((kind, build(s, kind)))
}

fn build(s: &Shell, kind: MenuKind) -> Menu {
    match kind {
        MenuKind::Keyboard => {
            let mut items: Vec<MenuItem> =
                s.d.layouts
                    .names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        MenuItem::new(i as u32, Label::key(format!("desk.kbd.name.{name}")))
                            .checked(i == usize::from(s.d.layouts.active))
                    })
                    .collect();
            let settings =
                MenuItem::new(MENU_KEYBOARD_SETTINGS, Label::key("desk.kbd.settings")).separated();
            items.push(if s.d.presence.launcher {
                settings
            } else {
                settings.disabled()
            });
            Menu::new(items)
        }
        MenuKind::Power => power_menu(s),
        MenuKind::Outputs => {
            let mut items: Vec<MenuItem> =
                s.d.svc
                    .outputs
                    .iter()
                    .enumerate()
                    .map(|(i, (_, name, active))| {
                        MenuItem::new(OUTPUT_BASE + i as u32, Label::data(name)).checked(*active)
                    })
                    .collect();
            if items.is_empty() {
                // Not known (yet): one disabled line instead of an empty menu.
                items.push(
                    MenuItem::new(OUTPUT_BASE + 99, Label::key("desk.qs.output.none")).disabled(),
                );
            }
            Menu::new(items)
        }
        MenuKind::Trash => Menu::new(vec![
            MenuItem::new(OPEN, Label::key("desk.menu.open")),
            MenuItem::new(EMPTY_TRASH, Label::key("desk.menu.empty_trash"))
                .danger()
                .disabled(),
        ]),
        MenuKind::Tile(i) => tile_menu(s, i),
        MenuKind::Drive(_) => Menu::new(vec![
            MenuItem::new(OPEN, Label::key("desk.menu.open")),
            MenuItem::new(EJECT, Label::key("desk.menu.eject")).separated(),
        ]),
        MenuKind::App => app_menu(s),
    }
}

/// shell.appmenu: New window, About (the app's page in the Store), App settings (its page
/// in Settings), Quit; the three that start an app need the launcher.
fn app_menu(s: &Shell) -> Menu {
    if !s.d.svc.mesh.targets.is_empty() {
        return super::handoff::targets_menu(s, HANDOFF, TARGET_BASE);
    }
    let gate = |item: MenuItem| {
        if s.d.presence.launcher {
            item
        } else {
            item.disabled()
        }
    };
    let mut items = vec![
        gate(MenuItem::new(NEW_WINDOW, Label::key("desk.menu.new_window")).icon(IconId::Plus)),
        gate(MenuItem::new(ABOUT, Label::key("desk.menu.about"))),
        gate(MenuItem::new(
            APP_SETTINGS,
            Label::key("desk.menu.app_settings"),
        )),
    ];
    // Shown only on a device with the mesh service.
    if s.d.svc.mesh.state.is_some() {
        items.push(super::handoff::item(s, HANDOFF));
    }
    items.push(MenuItem::new(QUIT, Label::key("desk.menu.quit")).separated());
    Menu::new(items)
}

/// qs.power: each entry enabled when its service offers it.
fn power_menu(s: &Shell) -> Menu {
    let (suspend, restart, off, sign_out) = s.d.svc.power_entries();
    let enabled = [suspend, restart, off, sign_out, true];
    Menu::new(
        POWER
            .iter()
            .zip(enabled)
            .enumerate()
            .map(|(i, (k, on))| {
                let item = MenuItem::new(POWER_BASE + i as u32, Label::key(*k));
                if on {
                    item
                } else {
                    item.disabled()
                }
            })
            .collect(),
    )
}

/// A chosen power menu entry.
fn power_chosen(s: &Shell, id: u32) -> Outcome {
    let call = match id.checked_sub(POWER_BASE) {
        Some(0) => Call::Suspend,
        Some(1) => Call::Restart,
        Some(2) => Call::PowerOff,
        Some(3) => Call::SignOut,
        // Switch user: the lock screen shows the accounts (session.switch_user from there).
        Some(4) => return Outcome::effect(Effect::Lock),
        _ => return Outcome::none(),
    };
    let _ = s;
    Outcome::effect(Effect::Service(call))
}

fn tile_menu(s: &Shell, index: usize) -> Menu {
    let tiles = s.d.bar_apps();
    let Some(tile) = tiles.get(index) else {
        return Menu::new(Vec::new());
    };
    let mut items: Vec<MenuItem> = tile
        .windows
        .iter()
        .enumerate()
        .take(16)
        .map(|(i, w)| {
            MenuItem::new(WINDOW_BASE + i as u32, Label::data(&w.title)).checked(w.focused)
        })
        .collect();
    let launcher = s.d.presence.launcher;
    let new = MenuItem::new(NEW_WINDOW, Label::key("desk.menu.new_window")).icon(IconId::Plus);
    items.push(if launcher {
        new.separated()
    } else {
        new.separated().disabled()
    });
    let pin_key = if tile.pinned {
        "desk.menu.unpin"
    } else {
        "desk.menu.pin"
    };
    items.push(MenuItem::new(PIN, Label::key(pin_key)));
    if !tile.windows.is_empty() {
        items.push(MenuItem::new(QUIT, Label::key("desk.menu.quit")).separated());
    }
    Menu::new(items)
}

/// The secondary button / menu key on a bar node.
pub(super) fn open_for(s: &mut Shell, target: &Target) -> Outcome {
    s.d.open = match target {
        Target::App(i) => Open::TileMenu(*i),
        Target::Trash => Open::TileMenu(usize::MAX),
        Target::Drive(i) => Open::DriveMenu(*i),
        Target::Keyboard => Open::KeyboardMenu,
        _ => return Outcome::none(),
    };
    Outcome::surfaces()
}

/// A chosen item.
pub(super) fn chosen(s: &mut Shell, id: u32) -> Outcome {
    let open = s.d.open;
    s.d.open = Open::None;
    if let Some(out) = super::handoff::chosen(s, open, id, HANDOFF, TARGET_BASE) {
        return out;
    }
    let mut out = match (open, id) {
        (Open::KeyboardMenu, MENU_KEYBOARD_SETTINGS) => {
            launch(s, SETTINGS, Opening::settings_page("keyboard"))
        }
        (Open::KeyboardMenu, i) if (i as usize) < s.d.layouts.names.len() => {
            Outcome::effect(Effect::Layout(i as u8))
        }
        (Open::PowerMenu, id) => power_chosen(s, id),
        (Open::DriveMenu(i), id) => match s.d.svc.drives.as_ref().and_then(|d| d.get(i)) {
            Some(drive) if id == EJECT => {
                Outcome::effect(Effect::Service(Call::Eject(drive.id.clone())))
            }
            Some(drive) if id == OPEN => {
                let uri = drive.uri.clone();
                launch(s, FILES, Opening::uri(uri))
            }
            _ => Outcome::none(),
        },
        (Open::OutputMenu, id) => match id
            .checked_sub(OUTPUT_BASE)
            .and_then(|i| s.d.svc.outputs.get(i as usize))
        {
            Some((output, _, _)) => {
                Outcome::effect(Effect::Service(Call::SetOutput(output.clone())))
            }
            None => Outcome::none(),
        },
        (Open::TileMenu(usize::MAX), OPEN) => launch(s, FILES, Opening::uri(TRASH_URI)),
        (Open::TileMenu(i), id) => tile_chosen(s, i, id),
        (Open::AppMenu, id) => {
            let active = s.d.bar_apps().iter().position(|t| t.active());
            match active {
                Some(i) if matches!(id, NEW_WINDOW | QUIT) => tile_chosen(s, i, id),
                Some(i) if matches!(id, ABOUT | APP_SETTINGS) => {
                    let tiles = s.d.bar_apps();
                    let Some(app) = tiles.get(i).map(|t| t.app.id.clone()) else {
                        return Outcome::none();
                    };
                    match id {
                        ABOUT => launch(s, STORE, Opening::store_page(&app, false)),
                        _ => launch(s, SETTINGS, Opening::settings_page(&format!("apps/{app}"))),
                    }
                }
                _ => Outcome::none(),
            }
        }
        _ => Outcome::none(),
    };
    out.bar = true;
    out.popup = true;
    out
}

fn tile_chosen(s: &mut Shell, index: usize, id: u32) -> Outcome {
    let tiles = s.d.bar_apps();
    let Some(tile) = tiles.get(index) else {
        return Outcome::none();
    };
    match id {
        NEW_WINDOW => launch(s, &tile.app.id.clone(), Opening::new_window()),
        PIN => {
            let app = tile.app.id.clone();
            if tile.pinned {
                s.d.pinned.retain(|p| *p != app);
            } else if !s.d.pinned.contains(&app) {
                s.d.pinned.push(app);
            }
            Outcome::surfaces()
        }
        QUIT => Outcome {
            effects: tile
                .windows
                .iter()
                .map(|w| Effect::Window(w.id, WindowAction::Close))
                .collect(),
            bar: true,
            popup: true,
        },
        w if w >= WINDOW_BASE => match tile.windows.get((w - WINDOW_BASE) as usize) {
            Some(win) => Outcome::effect(Effect::Window(win.id, WindowAction::Activate)),
            None => Outcome::none(),
        },
        _ => Outcome::none(),
    }
}
