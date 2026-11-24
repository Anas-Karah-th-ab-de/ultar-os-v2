//! Dialog and sheet: modal surfaces above everything else. While one is open, focus stays
//! inside it and nothing behind it reacts.

use super::Label;
use crate::geom::{Rect, Size};
use crate::layout::{Flex, LogicalRect, Padding};
use crate::paint::Painter;
use crate::tokens::{state, ColorRole, Radius, SizeClass, Space, ThemeKind};
use crate::ui::NodeId;

/// Where a modal surface sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DialogKind {
    /// Centred; a decision the user has to take. A press beside it does nothing.
    #[default]
    Dialog,
    /// Attached to an edge: the bottom on compact surfaces, the end edge otherwise. A press
    /// beside it dismisses it.
    Sheet,
    /// A floating panel next to the widget that opened it (shell sheets above the bar), at
    /// its end edge; no scrim. A press beside it dismisses it.
    Panel,
}

/// A dialog or sheet. Its children are laid out below its title; it is not part of its
/// parent's layout. Closed (the initial state) it is neither painted nor reachable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    /// The title (also the accessible name).
    pub title: Label,
    /// Dialog or sheet.
    pub kind: DialogKind,
    /// Layout of title and children.
    pub flex: Flex,
    pub(crate) open: bool,
    /// The button Enter activates.
    pub(crate) default_action: Option<NodeId>,
    /// The button Escape activates.
    pub(crate) cancel_action: Option<NodeId>,
    /// Where focus was before the dialog opened.
    pub(crate) return_focus: Option<NodeId>,
    /// The node a panel hangs on.
    pub(crate) anchor: Option<NodeId>,
    /// The node the dialog was added to: removing it removes the dialog (and closes it).
    pub(crate) host: Option<NodeId>,
    /// `true` for a dialog that asks about something serious (role `alertdialog`).
    pub alert: bool,
}

/// Widest dialog.
const DIALOG_W: f32 = 10.0 * Space::S48.points();
/// Width of a sheet at the end edge.
const SHEET_W: f32 = 8.0 * Space::S48.points();
/// Distance a dialog keeps from the surface edges.
const MARGIN: Space = Space::S24;

impl Dialog {
    /// A centred dialog.
    pub fn new(title: Label) -> Dialog {
        Dialog {
            title,
            kind: DialogKind::Dialog,
            flex: Flex::column()
                .padding(Padding::all(Space::S24))
                .gap(Space::S16),
            open: false,
            default_action: None,
            cancel_action: None,
            return_focus: None,
            anchor: None,
            host: None,
            alert: false,
        }
    }

    /// A floating panel; open it with `Ui::open_panel` next to its anchor.
    pub fn panel(title: Label) -> Dialog {
        Dialog {
            kind: DialogKind::Panel,
            flex: Flex::column()
                .padding(Padding::all(Space::S16))
                .gap(Space::S12),
            ..Dialog::new(title)
        }
    }

    /// The same dialog announced as an alert dialog.
    pub fn alert(mut self) -> Dialog {
        self.alert = true;
        self
    }

    /// `true` for surfaces that dim what is behind them.
    pub(crate) fn has_scrim(&self) -> bool {
        self.kind != DialogKind::Panel
    }

    /// A sheet.
    pub fn sheet(title: Label) -> Dialog {
        Dialog {
            kind: DialogKind::Sheet,
            ..Dialog::new(title)
        }
    }

    /// The same dialog with another layout of its content.
    pub fn flex(mut self, flex: Flex) -> Dialog {
        self.flex = flex;
        self
    }

    /// `true` while the dialog is shown.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// `true` for a sheet attached to the bottom edge (compact surfaces).
    fn at_bottom(&self, surface: Size) -> bool {
        self.kind == DialogKind::Sheet && SizeClass::for_width(surface.w) == SizeClass::Compact
    }

    /// The width the content is measured with.
    pub(crate) fn width(&self, surface: Size) -> f32 {
        let sides = MARGIN.points() + MARGIN.points();
        match self.kind {
            DialogKind::Dialog => DIALOG_W.min(surface.w - sides).max(0.0),
            DialogKind::Sheet if self.at_bottom(surface) => surface.w.max(0.0),
            DialogKind::Sheet => SHEET_W.min(surface.w).max(0.0),
            DialogKind::Panel => SHEET_W.min(surface.w - sides).max(0.0),
        }
    }

    /// The frame of the panel for content of natural size `content`.
    pub(crate) fn frame(
        &self,
        content: Size,
        surface: Size,
        anchor: Option<LogicalRect>,
    ) -> LogicalRect {
        let width = self.width(surface);
        match self.kind {
            DialogKind::Panel => panel_frame(width, content, surface, anchor),
            DialogKind::Dialog => {
                let height = content.h.min(surface.h - 2.0 * MARGIN.points()).max(0.0);
                let (start, top) = ((surface.w - width) / 2.0, (surface.h - height) / 2.0);
                LogicalRect::new(start, top, width, height)
            }
            DialogKind::Sheet if self.at_bottom(surface) => {
                let height = content.h.min(surface.h - MARGIN.points()).max(0.0);
                LogicalRect::new(0.0, surface.h - height, width, height)
            }
            DialogKind::Sheet => LogicalRect::new(surface.w - width, 0.0, width, surface.h),
        }
    }

    /// The colour laid over everything behind the dialog.
    pub(crate) fn scrim(p: &Painter<'_>) -> crate::color::Color {
        let alpha = match p.env().theme {
            ThemeKind::Dark => state::SCRIM_DARK,
            ThemeKind::Light => state::SCRIM_LIGHT,
        };
        p.color(ColorRole::Ground).with_alpha(alpha)
    }

    /// Paints the panel at `rect` (the scrim is painted once into the backdrop, see
    /// `Ui::paint`). Boards: the raised fill.
    pub(crate) fn paint(&self, p: &mut Painter<'_>, rect: Rect) {
        p.shadow(rect, Some(Radius::Card));
        let (panel, line, hairline) = (
            p.color(ColorRole::Raised),
            p.color(ColorRole::Line),
            p.hairline(),
        );
        p.fill(rect, Some(Radius::Card), panel);
        p.stroke(rect, Some(Radius::Card), hairline, line);
    }
}

/// A panel next to its anchor: at the anchor's end edge, above it when the anchor is in
/// the lower half of the surface (the bar), below it otherwise; inside the surface.
fn panel_frame(
    width: f32,
    content: Size,
    surface: Size,
    anchor: Option<LogicalRect>,
) -> LogicalRect {
    let margin = Space::S8.points();
    let height = content.h.min(surface.h - 2.0 * margin).max(0.0);
    let anchor = anchor.unwrap_or(LogicalRect::new(surface.w, surface.h, 0.0, 0.0));
    let end = anchor.start + anchor.width;
    let start = (end - width).min(surface.w - width - margin).max(margin);
    let above = anchor.top + anchor.height / 2.0 > surface.h / 2.0;
    let top = if above {
        anchor.top - margin - height
    } else {
        anchor.top + anchor.height + margin
    };
    let top = top.min(surface.h - height - margin).max(margin);
    LogicalRect::new(start, top, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogs_are_centred_and_sheets_hug_an_edge() {
        let wide = Size::new(1280.0, 800.0);
        let phone = Size::new(360.0, 720.0);
        let content = Size::new(0.0, 200.0);
        let dialog = Dialog::new(Label::key("d"));
        assert!(!dialog.is_open());
        let frame = dialog.frame(content, wide, None);
        assert_eq!((frame.width, frame.height), (DIALOG_W, 200.0));
        assert_eq!(frame.start, (wide.w - DIALOG_W) / 2.0);
        assert_eq!(frame.top, 300.0);
        let narrow = dialog.frame(content, phone, None);
        assert_eq!(narrow.width, phone.w - 2.0 * MARGIN.points());
        let tall = dialog.frame(Size::new(0.0, 5000.0), phone, None);
        assert_eq!(tall.height, phone.h - 2.0 * MARGIN.points());
        let sheet = Dialog::sheet(Label::key("s"));
        let side = sheet.frame(content, wide, None);
        assert_eq!(
            (side.start, side.top, side.height),
            (wide.w - SHEET_W, 0.0, wide.h)
        );
        let bottom = sheet.frame(content, phone, None);
        assert_eq!((bottom.start, bottom.width), (0.0, phone.w));
        assert_eq!(bottom.top + bottom.height, phone.h);
        let nothing = dialog.frame(content, Size::new(0.0, 0.0), None);
        let panel = Dialog::panel(Label::key("p"));
        let bar = LogicalRect::new(1100.0, 760.0, 40.0, 40.0);
        let above = panel.frame(content, wide, Some(bar));
        assert_eq!(
            (above.start + above.width, above.top + above.height),
            (1140.0, 752.0)
        );
        assert!(!panel.has_scrim() && dialog.has_scrim());
        assert_eq!((nothing.width, nothing.height), (0.0, 0.0));
    }
}
