//! Keys of the shell (DESKTOP-SPEC §5.4): Super = overview, Super+A = all apps,
//! Super+P = display modes (steps; Enter confirms, Esc cancels), Super+1…9 = app n of the
//! bar, Super+I = Settings, Super+E = Files, Super+V clipboard history (no service yet),
//! Esc closes what is open.

use super::bar::{launch, FILES, SETTINGS};
use super::Opening;
use super::{Effect, Outcome, Shell};
use crate::model::Open;
use crate::view::projection::MODES;
use ultar_ui::event::Key;

/// Linux key codes of the shortcuts.
mod code {
    pub const SUPER_L: u16 = 125;
    pub const SUPER_R: u16 = 126;
    pub const A: u16 = 30;
    pub const P: u16 = 25;
    pub const I: u16 = 23;
    pub const E: u16 = 18;
    pub const ONE: u16 = 2;
    pub const NINE: u16 = 10;
}

pub(super) fn on_shell_key(s: &mut Shell, key: u16) -> Outcome {
    match key {
        code::SUPER_L | code::SUPER_R => s.toggle(Open::Overview),
        code::A => s.toggle(Open::Apps(None)),
        code::P => step_projection(s),
        code::I => launch(s, SETTINGS, Opening::start()),
        code::E => launch(s, FILES, Opening::start()),
        n @ code::ONE..=code::NINE => {
            let index = usize::from(n - code::ONE);
            match s.d.bar_apps().get(index) {
                Some(tile) => {
                    let app = tile.app.id.clone();
                    match tile.windows.last() {
                        Some(w) => {
                            s.d.open = Open::None;
                            Outcome::effect_and_close(Effect::Window(
                                w.id,
                                ultar_surface::proto::WindowAction::Activate,
                            ))
                        }
                        None => launch(s, &app, Opening::start()),
                    }
                }
                None => Outcome::none(),
            }
        }
        _ => Outcome::none(),
    }
}

/// Super+P: opens the panel at the current mode, then steps through the four.
fn step_projection(s: &mut Shell) -> Outcome {
    s.d.open = match s.d.open {
        Open::Projection(mode) => Open::Projection((mode + 1) % MODES.len()),
        _ => Open::Projection(s.projection),
    };
    Outcome::surfaces()
}

pub(super) fn on_popup_key(s: &mut Shell, key: &Key) -> Outcome {
    match (s.d.open, key) {
        // Esc in the join dialog: back to the networks.
        (Open::Quick(crate::model::QuickPage::WifiJoin), Key::Escape) => super::network::cancel(s),
        (_, Key::Escape) => s.close(),
        (Open::Projection(_), Key::Enter) => confirm_projection(s),
        _ => Outcome::none(),
    }
}

/// Enter or a click: the highlighted mode; the "keep?" dialog follows from the settings
/// service.
pub(super) fn confirm_projection(s: &mut Shell) -> Outcome {
    let Open::Projection(mode) = s.d.open else {
        return Outcome::none();
    };
    s.projection = mode;
    s.d.open = Open::None;
    Outcome::effect_and_close(Effect::DisplayMode(mode))
}
