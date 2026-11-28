//! The toasts of the mesh (board MeshStates, docs/ui/mesh.md §2.6): the toolkit's toast –
//! one line with the tone's icon, an optional button (Cancel, Retry, Open) and Close – in a
//! surface of its own above the bar, centred. The toolkit times them out (ten seconds with a
//! button, longer while the pointer or the focus is on them) and reports the button and the
//! closing; the shell answers through the root node ([`Target::MeshToasts`]).

use super::geometry::Placement;
use super::{new_ui, Look, Target, View};
use crate::model::mesh::Toast as Shown;
use crate::model::Desktop;
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::tokens::{size, Space};
use ultar_ui::ui::UiError;
use ultar_ui::widgets::{Label, Toast};

/// Space around the toasts inside the surface (the toolkit keeps its toasts 16 from the edge).
const EDGE: f32 = 16.0;
/// Gap between two toasts (the toolkit's).
const GAP: f32 = Space::S8.points();
/// Distance of the surface from the bar.
const ABOVE_BAR: f32 = Space::S8.points();

/// The toolkit toast of a mesh toast.
fn toolkit(t: &Shown) -> Toast {
    let args = [("name", t.name.as_str()), ("title", t.title.as_str())];
    let toast = Toast::new(t.id, t.kind.tone(), Label::key_with(t.kind.key(), &args));
    match &t.act {
        Some(act) => toast.action(Label::key(act.key())),
        None => toast,
    }
}

/// Builds the toasts' surface at `now_ms` (the host's clock: the toasts' time starts now);
/// `None` without a toast. The surface is placed relative to the bar's panel; `top_bar`
/// says that panel is at the top of the screen (the toasts stay at the screen's bottom).
pub fn build(
    d: &Desktop,
    (look, screen, now_ms): (Look, Size, u64),
    top_bar: bool,
    tr: &Translator,
) -> Result<Option<(View, Placement)>, UiError> {
    let toasts = &d.svc.mesh.toasts;
    if toasts.is_empty() {
        return Ok(None);
    }
    let each = look.mode.min_target() + 2.0 * Space::S8.points();
    let count = toasts.len() as f32;
    let height = 2.0 * EDGE + count * each + (count - 1.0) * GAP;
    let width = (size::TOAST_WIDTH + 2.0 * EDGE).min(screen.w);
    let surface = Size::new(width, height);
    let mut ui = new_ui(look, surface, tr)?;
    ui.set_transparent_background();
    ui.advance_to(now_ms);
    for t in toasts {
        ui.show_toast(toolkit(t))?;
    }
    let root = ui.root();
    let mut view = View::new(ui);
    view.mark(root, Target::MeshToasts);
    let x = ((screen.w - width) / 2.0).max(0.0);
    let y = if top_bar {
        screen.h - ABOVE_BAR - height
    } else {
        -(height + ABOVE_BAR)
    };
    let place = Placement {
        offset: (x.round() as i32, y.round() as i32),
        surface,
        start: Space::S4,
        top: Space::S4,
    };
    Ok(Some((view, place)))
}
