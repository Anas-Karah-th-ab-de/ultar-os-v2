//! Notification banners (board Toasts): at most three cards at the top of the screen on the
//! end side (as the bar's sheets), newest on top; a normal one goes after five seconds while
//! the pointer is not on it, an important one stays until it is closed.

use super::geometry::{self, Placement};
use super::note::{self, Style};
use super::{new_ui, Look, View};
use crate::model::Desktop;
use ultar_i18n::Translator;
use ultar_ui::geom::{Rect, Size};
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::UiError;

/// Width of a banner (board: 400).
pub const WIDTH: f32 = 100.0 * Space::S4.points();
/// Distance from the screen edges.
pub const INSET: Space = Space::S16;

/// Builds the banners shown at `now` (`None` without any).
pub fn build(
    d: &Desktop,
    now: (u64, i64),
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<Option<(View, Placement)>, UiError> {
    let shown = d.notes.banners(now.0);
    if shown.is_empty() {
        return Ok(None);
    }
    let mut ui = new_ui(look, screen, tr)?;
    ui.set_transparent_background();
    ui.set_root_flex(Flex::column().cross(CrossAlign::Start));
    let root = ui.root();
    // Narrower on a small screen (tokens: `toast_width`, "narrower on a small surface").
    let width = WIDTH.min((screen.w - 2.0 * INSET.points()).max(0.0));
    let column = ui.add_sized(root, Flex::column().gap(Space::S8), Sizing::width(width))?;
    let mut view = View::new(ui);
    for n in shown {
        note::card(&mut view, column, (d, n), now, Style::Banner, tr)?;
    }
    let height = view.ui.content_size().h;
    let rtl = look.lang.direction().is_rtl();
    // The end side, like the sheets of the bar: left in Arabic, right in English (board
    // Toasts, Arabic: top left).
    let x = if rtl {
        INSET.points()
    } else {
        screen.w - INSET.points() - width
    };
    let place = geometry::place(screen, Rect::new(x, INSET.points(), width, height), rtl);
    view.ui
        .set_root_flex(Flex::column().cross(CrossAlign::Start).padding(Padding {
            start: Some(place.start),
            end: None,
            top: Some(place.top),
            bottom: None,
        }));
    view.ui.set_size(place.surface)?;
    Ok(Some((view, place)))
}
