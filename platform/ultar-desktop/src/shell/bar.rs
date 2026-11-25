//! Clicks on the bar (CLICKS.md: Main / DesktopBottom, `shell.*`).

use super::Opening;
use super::{Effect, Outcome, Shell};
use crate::model::{Open, QuickPage};
use crate::view::Target;
use ultar_surface::proto::{WindowAction, WindowState};
use ultar_ui::event::ActionKind;

/// App id of Files (trash, drives).
pub const FILES: &str = "org.ultar.files";
/// The trash, as Files opens it (`[open] schemes` of Files).
pub const TRASH_URI: &str = "trash:///";
/// App id of Settings.
pub const SETTINGS: &str = "org.ultar.settings";
/// The Calendar app (appointments, `calendar://event/<id>`).
pub const CALENDAR: &str = "org.ultar.calendar";
/// The Store (app pages and removal, `store://app/<id>`).
pub const STORE: &str = "org.ultar.store";

pub(super) fn on_bar(s: &mut Shell, target: &Target, kind: &ActionKind) -> Outcome {
    match (target, kind) {
        (Target::App(i), ActionKind::MiddleClicked) => new_window(s, *i),
        // shell.status.network: a click on the network symbol (always the first status
        // icon) opens the Wi-Fi page; on another symbol, the tiles.
        (Target::Status, ActionKind::Selected(0)) => super::network::wifi_from_bar(s),
        (Target::Status, ActionKind::Selected(_)) => activated(s, target),
        (_, ActionKind::Activated) => activated(s, target),
        _ => Outcome::none(),
    }
}

fn activated(s: &mut Shell, target: &Target) -> Outcome {
    match target {
        // shell.trash: the trash in Files.
        Target::Trash => launch(s, FILES, Opening::uri(TRASH_URI)),
        Target::Drive(i) => match s.d.svc.drives.as_ref().and_then(|d| d.get(*i)) {
            Some(drive) => {
                let uri = drive.uri.clone();
                launch(s, FILES, Opening::uri(uri))
            }
            None => Outcome::none(),
        },
        // shell.overview: opens; a second click closes.
        Target::Overview => s.toggle(Open::Overview),
        // shell.apps: the app grid.
        Target::AllApps => s.toggle(Open::Apps(None)),
        // shell.search: the overview with the caret in its field (never closes it).
        Target::Search => {
            s.d.open = Open::Overview;
            Outcome {
                effects: s.overview_thumbnails().into_iter().collect(),
                ..Outcome::surfaces()
            }
        }
        Target::App(i) => app_clicked(s, *i),
        Target::Keyboard => s.toggle(Open::KeyboardMenu),
        // shell.appmenu: the menu of the active app.
        Target::AppMenu => s.toggle(Open::AppMenu),
        Target::Status => s.toggle(Open::Quick(QuickPage::Tiles)),
        Target::Clock => {
            // The sheet lists every notification: the banners go.
            s.d.notes.hide_banners();
            s.toggle(Open::Calendar)
        }
        _ => Outcome::none(),
    }
}

/// Starts an app (the click's token goes with it); closes what is open.
pub(super) fn launch(s: &mut Shell, app: &str, open: Opening) -> Outcome {
    s.d.open = Open::None;
    Outcome::effect_and_close(Effect::Launch {
        app: app.to_owned(),
        open,
    })
}

/// shell.app click: start it, raise its window, or – with several windows – choose one.
fn app_clicked(s: &mut Shell, index: usize) -> Outcome {
    let tiles = s.d.bar_apps();
    let Some(tile) = tiles.get(index) else {
        return Outcome::none();
    };
    match tile.windows.as_slice() {
        [] => {
            let app = tile.app.id.clone();
            launch(s, &app, Opening::start())
        }
        [w] => {
            // Its only window: raise it, or minimise it when it is in front (taskbar habit).
            let action = if w.focused && w.state != WindowState::Minimized {
                WindowAction::Minimize
            } else {
                WindowAction::Activate
            };
            s.d.open = Open::None;
            Outcome::effect_and_close(Effect::Window(w.id, action))
        }
        _ => {
            s.d.open = Open::TileMenu(index);
            Outcome::surfaces()
        }
    }
}

/// Middle click: a new window of the app.
fn new_window(s: &mut Shell, index: usize) -> Outcome {
    match s.d.bar_apps().get(index) {
        Some(tile) => {
            let app = tile.app.id.clone();
            launch(s, &app, Opening::new_window())
        }
        None => Outcome::none(),
    }
}
