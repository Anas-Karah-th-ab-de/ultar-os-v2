//! Clicks on the bar (CLICKS.md: Main / DesktopBottom, `shell.*`).

use super::{Effect, Outcome, Shell};
use crate::model::{Open, QuickPage};
use crate::view::Target;
use ultar_surface::proto::{WindowAction, WindowState};
use ultar_ui::event::ActionKind;

/// App id of Files (trash, drives).
pub const FILES: &str = "org.ultar.files";
/// App id of Settings.
pub const SETTINGS: &str = "org.ultar.settings";

pub(super) fn on_bar(s: &mut Shell, target: &Target, kind: &ActionKind) -> Outcome {
    match (target, kind) {
        (Target::App(i), ActionKind::MiddleClicked) => new_window(s, *i),
        (_, ActionKind::Activated) => activated(s, target),
        _ => Outcome::none(),
    }
}

fn activated(s: &mut Shell, target: &Target) -> Outcome {
    match target {
        // shell.trash: the trash in Files.
        Target::Trash => launch(s, FILES, &["trash:///"]),
        // shell.overview: opens; a second click closes.
        Target::Overview => s.toggle(Open::Overview),
        // shell.apps: the app grid.
        Target::AllApps => s.toggle(Open::Apps(None)),
        // shell.search: the overview with the caret in its field (never closes it).
        Target::Search => {
            s.d.open = Open::Overview;
            Outcome::surfaces()
        }
        Target::App(i) => app_clicked(s, *i),
        Target::Keyboard => s.toggle(Open::KeyboardMenu),
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
pub(super) fn launch(s: &mut Shell, app: &str, args: &[&str]) -> Outcome {
    s.d.open = Open::None;
    Outcome::effect_and_close(Effect::Launch {
        app: app.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
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
            launch(s, &app, &[])
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
            launch(s, &app, &["--new-window"])
        }
        None => Outcome::none(),
    }
}
