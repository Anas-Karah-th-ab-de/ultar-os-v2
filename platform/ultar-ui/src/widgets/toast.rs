//! Toast: a short message at the bottom of the surface that goes away by itself (status,
//! never an alert). It may offer one action. Shown with `Ui::show_toast`.

use super::style;
use super::{Label, MeasureCx, PaintCx, Tone};
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, InputMode, Radius, Space};
use ultar_i18n::Direction;

/// Most toasts visible at once; a further one replaces the oldest.
pub const MAX_TOASTS: usize = 3;

/// A toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    /// Identifier reported in `ToastAction` and `ToastClosed` (chosen by the application).
    pub id: u32,
    /// The meaning (icon and spoken name).
    pub tone: Tone,
    /// The message in one line.
    pub message: Label,
    /// The label of the action button. A toast with an action stays until it is closed.
    pub action: Option<Label>,
}

const PAD: Space = Space::S12;
const GAP: Space = Space::S8;
const EDGE: Space = Space::S16;
const MAX_W: f32 = 10.0 * Space::S48.points();

/// Where a shown toast and its parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ToastView {
    pub(crate) rect: Rect,
    pub(crate) action: Option<Rect>,
    pub(crate) close: Rect,
}

impl Toast {
    /// A toast with a message.
    pub fn new(id: u32, tone: Tone, message: Label) -> Toast {
        Toast {
            id,
            tone,
            message,
            action: None,
        }
    }

    /// The same toast with an action button.
    pub fn action(mut self, action: Label) -> Toast {
        self.action = Some(action);
        self
    }

    /// Height of a toast for an input mode.
    pub(crate) fn height(mode: InputMode) -> f32 {
        mode.min_target() + 2.0 * Space::S8.points()
    }

    /// Geometry of the toast that is `index` places above the lowest one.
    pub(crate) fn view(&self, cx: &mut MeasureCx<'_>, surface: Size, index: usize) -> ToastView {
        let height = Toast::height(cx.mode);
        let width = MAX_W.min(surface.w - 2.0 * EDGE.points()).max(0.0);
        let x = (surface.w - width) / 2.0;
        let step = height + GAP.points();
        let y = surface.h - EDGE.points() - height - index as f32 * step;
        let rect = Rect::new(x, y, width, height);
        let direction = cx.env.direction;
        let mut inner = style::inset_inline(rect, PAD);
        let target = cx.mode.min_target();
        let close = style::take_end(&mut inner, target, direction);
        let close = Rect::new(close.x, rect.y + (height - target) / 2.0, target, target);
        let action = self.action.as_ref().map(|label| {
            let width = cx.text_size(label, style::BUTTON_TEXT, None).w + 2.0 * PAD.points();
            let slot = style::take_end(&mut inner, width, direction);
            Rect::new(slot.x, close.y, slot.w, target)
        });
        ToastView {
            rect,
            action,
            close,
        }
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, view: &ToastView) {
        p.shadow(view.rect, Some(Radius::Card));
        let (bg, tone, border) = (
            p.color(ColorRole::Raised),
            p.color(self.tone.role()),
            p.hairline(),
        );
        p.fill(view.rect, Some(Radius::Card), bg);
        p.stroke(view.rect, Some(Radius::Card), border, tone);
        let direction: Direction = p.env().direction;
        let mut inner = style::inset_inline(view.rect, PAD);
        let used = view.close.w + view.action.map_or(0.0, |a| a.w);
        style::take_end(&mut inner, used, direction);
        let slot = style::take_start(&mut inner, size::ICON, direction);
        style::take_start(&mut inner, GAP.points(), direction);
        p.icon(self.tone.icon(), slot, size::ICON, QuarterTurns::None, tone);
        let text = p.color(ColorRole::Text);
        let spec = style::CONTROL_TEXT;
        style::draw_label(p, cx, &self.message, spec, inner, TextAlign::Start, text);
        if let (Some(rect), Some(label)) = (view.action, &self.action) {
            let accent = p.color(ColorRole::AccentText);
            let spec = style::CONTROL_TEXT.weight(Weight::Semibold);
            style::draw_label(p, cx, label, spec, rect, TextAlign::Center, accent);
        }
        let muted = p.color(ColorRole::TextMuted);
        let edge = size::ICON_SMALL;
        p.icon(IconId::Close, view.close, edge, QuarterTurns::None, muted);
    }
}
