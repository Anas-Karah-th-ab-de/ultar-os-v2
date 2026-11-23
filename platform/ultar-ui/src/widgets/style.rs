//! Shared look of the widgets: state colours, the focus ring, direction-aware slicing.
//! Every value is derived from the tokens.

use super::{Label, PaintCx};
use crate::color::Color;
use crate::geom::{Rect, Size};
use crate::paint::{Painter, TextSpec};
use crate::text::{Overflow, TextAlign, Weight};
use crate::tokens::{size, state, typography, ColorRole, Radius, Space, TypeSize};
use ultar_i18n::Direction;

/// Opacity of a disabled widget.
pub(crate) const DISABLED_OPACITY: f32 = state::DISABLED_OPACITY;
/// Share of the foreground mixed into a filled background while hovered.
pub(crate) const HOVER_MIX: f32 = state::HOVER_OVERLAY;
/// Share of the foreground mixed into a filled background while pressed.
pub(crate) const PRESSED_MIX: f32 = state::PRESSED_OVERLAY;

/// How far the focus ring reaches outside a widget (gap plus ring).
pub(crate) const fn focus_outset() -> f32 {
    2.0 * size::FOCUS_RING
}

/// Text of controls (buttons, fields, values).
pub(crate) const CONTROL_TEXT: TextSpec =
    TextSpec::new(typography::BODY).overflow(Overflow::Ellipsis);
/// Emphasised control text (button labels).
pub(crate) const BUTTON_TEXT: TextSpec = CONTROL_TEXT.weight(Weight::Semibold);
/// Secondary text (sublabels, error lines).
pub(crate) const SMALL_TEXT: TextSpec = TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap);

/// Interaction and status flags a widget is painted with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Visual {
    /// The pointer is over the widget.
    pub hovered: bool,
    /// The widget is held down.
    pub pressed: bool,
    /// The widget has keyboard focus and shows the focus ring.
    pub focused: bool,
    /// The widget cannot be operated.
    pub disabled: bool,
    /// The widget waits for a result.
    pub loading: bool,
    /// The widget's value is invalid.
    pub error: bool,
}

impl Visual {
    /// Opacity the widget is painted with: dimmed when disabled.
    pub(crate) fn opacity(&self) -> f32 {
        if self.disabled {
            DISABLED_OPACITY
        } else {
            1.0
        }
    }
}

/// Background role of a neutral control for its state: raised, hover, pressed.
pub(crate) fn neutral_background(v: &Visual) -> ColorRole {
    if v.pressed {
        ColorRole::Line
    } else if v.hovered {
        ColorRole::Hover
    } else {
        ColorRole::Raised
    }
}

/// A filled colour with its state layer: the foreground mixed in while hovered or pressed.
pub(crate) fn filled(base: Color, foreground: Color, v: &Visual) -> Color {
    if v.pressed {
        base.mix(foreground, PRESSED_MIX)
    } else if v.hovered {
        base.mix(foreground, HOVER_MIX)
    } else {
        base
    }
}

/// Draws the focus ring around `rect`. `ring` is the radius of the ring itself.
pub(crate) fn focus_ring(p: &mut Painter<'_>, rect: Rect, ring: Radius) {
    let color = p.color(state::FOCUS_RING);
    p.stroke(
        rect.inflate(focus_outset()),
        Some(ring),
        size::FOCUS_RING,
        color,
    );
}

/// Cuts `width` off the start side of `rect` and returns it; `rect` keeps the remainder.
pub(crate) fn take_start(rect: &mut Rect, width: f32, direction: Direction) -> Rect {
    let width = width.min(rect.w).max(0.0);
    let slice = if direction.is_rtl() {
        Rect::new(rect.right() - width, rect.y, width, rect.h)
    } else {
        let slice = Rect::new(rect.x, rect.y, width, rect.h);
        rect.x += width;
        slice
    };
    rect.w -= width;
    slice
}

/// Cuts `width` off the end side of `rect` and returns it; `rect` keeps the remainder.
pub(crate) fn take_end(rect: &mut Rect, width: f32, direction: Direction) -> Rect {
    take_start(rect, width, direction.opposite())
}

/// `rect` without `inline` points at start and end.
pub(crate) fn inset_inline(rect: Rect, inline: Space) -> Rect {
    let d = inline.points().min(rect.w / 2.0);
    Rect::new(rect.x + d, rect.y, rect.w - 2.0 * d, rect.h)
}

/// A box of `size` centred in `rect`.
pub(crate) fn centered(rect: Rect, size: Size) -> Rect {
    Rect::new(
        rect.x + (rect.w - size.w) / 2.0,
        rect.y + (rect.h - size.h) / 2.0,
        size.w,
        size.h,
    )
}

/// Draws a label into `rect` (vertically centred) and returns its width in points.
pub(crate) fn draw_label(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    label: &Label,
    spec: TextSpec,
    rect: Rect,
    align: TextAlign,
    color: Color,
) -> f32 {
    let text = label.resolve(cx.tr);
    let layout = p.layout(&text, spec, Some(rect.w.max(0.0)));
    p.text(&layout, rect, align, color);
    layout.width / p.env().scale
}

/// Height of the error line under a field of `width`, including its gap; 0 without error.
pub(crate) fn error_height(
    cx: &mut super::MeasureCx<'_>,
    error: Option<&Label>,
    width: f32,
) -> f32 {
    match error {
        Some(label) => Space::S4.points() + cx.text_size(label, SMALL_TEXT, Some(width)).h,
        None => 0.0,
    }
}

/// Draws the error line into the part of `rect` below the field of height `field_height`.
pub(crate) fn paint_error(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    error: Option<&Label>,
    rect: Rect,
    field_height: f32,
) {
    if let Some(label) = error {
        let top = rect.y + field_height + Space::S4.points();
        let area = Rect::new(rect.x, top, rect.w, (rect.bottom() - top).max(0.0));
        let color = p.color(ColorRole::Danger);
        draw_label(p, cx, label, SMALL_TEXT, area, TextAlign::Start, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::DARK;

    #[test]
    fn slicing_follows_the_direction() {
        let full = Rect::new(0.0, 0.0, Space::S48.points(), Space::S24.points());
        let mut rest = full;
        let start = take_start(&mut rest, Space::S8.points(), Direction::Ltr);
        assert_eq!(
            (start.x, rest.x, rest.w),
            (
                0.0,
                Space::S8.points(),
                Space::S48.points() - Space::S8.points()
            )
        );
        let mut rest = full;
        let start = take_start(&mut rest, Space::S8.points(), Direction::Rtl);
        assert_eq!((start.right(), rest.x), (full.right(), 0.0));
        let mut rest = full;
        let end = take_end(&mut rest, Space::S8.points(), Direction::Rtl);
        assert_eq!((end.x, rest.x), (0.0, Space::S8.points()));
        let mut rest = full;
        let all = take_start(&mut rest, full.w * 2.0, Direction::Ltr);
        assert_eq!((all.w, rest.w), (full.w, 0.0));
        assert_eq!(
            inset_inline(full, Space::S4).w,
            full.w - 2.0 * Space::S4.points()
        );
        let c = centered(full, Size::new(Space::S8.points(), Space::S8.points()));
        assert_eq!(c.center(), full.center());
    }

    #[test]
    fn state_colours() {
        let idle = Visual::default();
        let hovered = Visual {
            hovered: true,
            ..idle
        };
        let pressed = Visual {
            pressed: true,
            hovered: true,
            ..idle
        };
        assert_eq!(neutral_background(&idle), ColorRole::Raised);
        assert_eq!(neutral_background(&hovered), ColorRole::Hover);
        assert_eq!(neutral_background(&pressed), ColorRole::Line);
        let (base, fg) = (DARK.get(ColorRole::Accent), DARK.get(ColorRole::OnAccent));
        assert_eq!(filled(base, fg, &idle), base);
        assert_ne!(filled(base, fg, &hovered), base);
        assert_ne!(filled(base, fg, &pressed), filled(base, fg, &hovered));
        assert!(focus_outset() <= Space::S4.points());
    }
}
