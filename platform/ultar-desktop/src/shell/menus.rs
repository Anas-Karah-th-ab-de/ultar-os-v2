//! Menus of the bar: the input languages (shell.kbd), the power menu (qs.power), the menu
//! of an app tile (shell.app right click: its windows, new window, pin/unpin, quit) and of
//! the trash (open, empty – the files service does not exist yet).

use super::bar::{launch, FILES};
use super::{Effect, Outcome, Shell};
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
    /// An app tile.
    Tile(usize),
    /// The trash tile.
    Trash,
}

/// Item ids: layouts are 0..8, windows are `WINDOW_BASE + index`.
pub const MENU_KEYBOARD_SETTINGS: u32 = 100;
const NEW_WINDOW: u32 = 101;
const PIN: u32 = 102;
const QUIT: u32 = 103;
const OPEN: u32 = 104;
const EMPTY_TRASH: u32 = 105;
const POWER_BASE: u32 = 200;
const WINDOW_BASE: u32 = 300;

/// Power menu entries; all need a power service that is not on the bus yet.
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
        Open::TileMenu(usize::MAX) => MenuKind::Trash,
        Open::TileMenu(i) => MenuKind::Tile(i),
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
        MenuKind::Power => Menu::new(
            POWER
                .iter()
                .enumerate()
                .map(|(i, k)| MenuItem::new(POWER_BASE + i as u32, Label::key(*k)).disabled())
                .collect(),
        ),
        MenuKind::Trash => Menu::new(vec![
            MenuItem::new(OPEN, Label::key("desk.menu.open")),
            MenuItem::new(EMPTY_TRASH, Label::key("desk.menu.empty_trash"))
                .danger()
                .disabled(),
        ]),
        MenuKind::Tile(i) => tile_menu(s, i),
    }
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
        Target::Keyboard => Open::KeyboardMenu,
        _ => return Outcome::none(),
    };
    Outcome::surfaces()
}

/// A chosen item.
pub(super) fn chosen(s: &mut Shell, id: u32) -> Outcome {
    let open = s.d.open;
    s.d.open = Open::None;
    let mut out = match (open, id) {
        (Open::KeyboardMenu, MENU_KEYBOARD_SETTINGS) => {
            launch(s, super::bar::SETTINGS, &["keyboard"])
        }
        (Open::KeyboardMenu, i) if (i as usize) < s.d.layouts.names.len() => {
            Outcome::effect(Effect::Layout(i as u8))
        }
        (Open::TileMenu(usize::MAX), OPEN) => launch(s, FILES, &["trash:///"]),
        (Open::TileMenu(i), id) => tile_chosen(s, i, id),
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
        NEW_WINDOW => launch(s, &tile.app.id.clone(), &["--new-window"]),
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
