//! The phone's edge gestures (mobile.home, DESKTOP-SPEC §5.5): the compositor recognises a
//! swipe from a screen edge in phone mode and tells only the shell (`EdgeSwipe`). Up from
//! the bottom edge: home; up and held: recent apps; inwards from a side edge: back – the
//! shell's own sheet or dialog first, otherwise the focused app (`WindowAction::Back`).

use super::{Effect, Outcome, Shell};
use crate::model::{Open, QuickPage};
use ultar_surface::proto::{SwipeEdge, WindowAction};

impl Shell {
    /// An edge gesture of the compositor (phone mode only).
    pub fn on_edge(&mut self, edge: SwipeEdge, held: bool) -> Outcome {
        if !self.shape.is_compact() {
            return Outcome::none();
        }
        match (edge, held) {
            (SwipeEdge::Bottom, false) => super::bar::home(self),
            (SwipeEdge::Bottom, true) => {
                self.d.open = Open::Overview;
                Outcome::surfaces()
            }
            (SwipeEdge::Side, _) => self.back(),
        }
    }

    /// Back: the join dialog to the networks, a list page to the tiles, an open sheet
    /// closes; with nothing open the focused app goes back one step.
    fn back(&mut self) -> Outcome {
        match self.d.open {
            Open::Quick(QuickPage::WifiJoin) => super::network::cancel(self),
            Open::Quick(QuickPage::Tiles) => self.close(),
            Open::Quick(_) => {
                self.d.open = Open::Quick(QuickPage::Tiles);
                Outcome::surfaces()
            }
            Open::None => match self.d.windows.iter().find(|w| w.focused) {
                Some(w) => Outcome::effect(Effect::Window(w.id, WindowAction::Back)),
                None => Outcome::none(),
            },
            _ => self.close(),
        }
    }
}
