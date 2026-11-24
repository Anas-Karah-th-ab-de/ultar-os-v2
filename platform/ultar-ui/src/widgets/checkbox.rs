//! Checkbox: an independent yes/no choice with its label (boards: `checkrow`).

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, typography, ColorRole, Radius, Space, TypeSize};

/// A checkbox. "Checked" is shown by the check mark, "mixed" by a dash – never by colour
/// alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkbox {
    /// The label (also the accessible name).
    pub label: Label,
    /// Current state.
    pub checked: bool,
    /// Neither checked nor unchecked (some of the things it stands for are on). Activating a
    /// mixed checkbox checks it.
    pub mixed: bool,
    /// Second line under the label (boards: `checkrow` with a sub text, 12, muted).
    pub sublabel: Option<Label>,
    /// Icon between the box and the texts (boards: 24, muted).
    pub icon: Option<IconId>,
    /// `true` for a check row as wide as its parent (boards: `checkrow`, `width: 100%`);
    /// such a row may host one trailing child (badge, value) at its end.
    pub wide: bool,
}

const BOX: f32 = size::BOX;
const GAP: Space = Space::S12;
/// Height of a check row (boards: 48).
const ROW: f32 = Space::S48.points();

impl Checkbox {
    /// A checkbox in the given state.
    pub fn new(label: Label, checked: bool) -> Checkbox {
        Checkbox {
            label,
            checked,
            mixed: false,
            sublabel: None,
            icon: None,
            wide: false,
        }
    }

    /// The same checkbox as a whole-width row with a second line (boards: component rows of
    /// the installer). The row is at least 48 high, padded 8 / 12, its texts wrap.
    pub fn sublabel(mut self, sublabel: Label) -> Checkbox {
        self.sublabel = Some(sublabel);
        self.wide = true;
        self
    }

    /// The same checkbox as a whole-width row with an icon before its texts.
    pub fn icon(mut self, icon: IconId) -> Checkbox {
        self.icon = Some(icon);
        self.wide = true;
        self
    }

    /// The same checkbox as wide as its parent (it may then host one trailing child).
    pub fn wide(mut self) -> Checkbox {
        self.wide = true;
        self
    }

    /// Width left for the texts of a wide row of `width` with a trailing child of `control`.
    fn text_width(&self, width: f32, control: f32) -> f32 {
        let icon = if self.icon.is_some() {
            size::ICON + GAP.points()
        } else {
            0.0
        };
        let end = if control > 0.0 {
            control + GAP.points()
        } else {
            0.0
        };
        (width - 3.0 * GAP.points() - BOX - icon - end).max(0.0)
    }

    /// Height of the texts of a wide row (label, sublabel).
    fn text_heights(&self, cx: &mut MeasureCx<'_>, width: f32) -> (f32, f32) {
        let spec = style::CONTROL_TEXT
            .weight(Weight::Medium)
            .overflow(crate::text::Overflow::Wrap);
        // Lines × the exact line height of the type tokens (boards: CSS line boxes are not
        // rounded, so a column of rows does not drift from the board).
        let exact = |cx: &mut MeasureCx<'_>, h: f32, size: TypeSize| {
            let line = super::list::line_box(cx.env, size);
            let factor = if cx.env.lang == ultar_i18n::Lang::Ar {
                typography::LINE_HEIGHT_ARABIC
            } else {
                typography::LINE_HEIGHT_LATIN
            };
            (h / line).round().max(1.0) * size.points() * factor
        };
        let h = cx.text_size(&self.label, spec, Some(width)).h;
        let label = exact(cx, h, TypeSize::S14);
        let sub = match &self.sublabel {
            Some(s) => {
                let h = cx.text_size(s, style::SMALL_TEXT, Some(width)).h;
                exact(cx, h, TypeSize::S12)
            }
            None => 0.0,
        };
        (label, sub)
    }

    /// Placement of the trailing child of a wide row of `size`: (from the start, from the top).
    pub(crate) fn control_offset(size: Size, control: Size) -> (f32, f32) {
        (
            size.w - GAP.points() - control.w,
            (size.h - control.h) / 2.0,
        )
    }

    /// The same checkbox in the mixed state.
    pub fn mixed(mut self) -> Checkbox {
        self.mixed = true;
        self
    }

    /// Flips the state (a mixed box becomes checked); returns the new state.
    pub(crate) fn toggle(&mut self) -> bool {
        self.checked = self.mixed || !self.checked;
        self.mixed = false;
        self.checked
    }

    /// The row part of the node (an error line may follow below it).
    pub(crate) fn row(rect: Rect, cx_min: f32) -> Rect {
        Rect::new(rect.x, rect.y, rect.w, ROW.max(cx_min).min(rect.h))
    }

    pub(crate) fn measure(
        &self,
        cx: &mut MeasureCx<'_>,
        avail: f32,
        error: Option<&Label>,
        control: Size,
    ) -> Size {
        if self.wide {
            let (label, sub) = self.text_heights(cx, self.text_width(avail, control.w));
            let pad = 2.0 * Space::S8.points();
            let row = (label + sub + pad)
                .max(control.h + pad)
                .max(ROW)
                .max(cx.mode.min_target());
            return Size::new(avail, row + style::error_height(cx, error, avail));
        }
        let spec = style::CONTROL_TEXT.weight(Weight::Medium);
        let text = cx.text_size(&self.label, spec, None);
        let natural = BOX + 3.0 * GAP.points() + text.w;
        let width = natural.min(avail).max(BOX);
        let row = ROW.max(cx.mode.min_target());
        Size::new(width, row + style::error_height(cx, error, width))
    }

    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        error: Option<&Label>,
        control: f32,
    ) {
        let previous = p.set_opacity(v.opacity());
        let row = if self.wide {
            Rect::new(
                rect.x,
                rect.y,
                rect.w,
                Checkbox::row_height(p, cx, rect, error),
            )
        } else {
            Checkbox::row(rect, cx.mode.min_target())
        };
        if v.hovered || v.pressed {
            let bg = p.color(style::neutral_background(v));
            p.fill(row, Some(Radius::Control), bg);
        }
        let direction = p.env().direction;
        // Boards: padding 12 at both ends, so the focus ring never touches box or label.
        let mut inner = style::inset_inline(row, GAP);
        let slot = style::take_start(&mut inner, BOX, direction);
        style::take_start(&mut inner, GAP.points(), direction);
        let mark = style::centered(slot, Size::new(BOX, BOX));
        self.paint_box(p, mark, v);
        if v.loading {
            // Boards: the box keeps its state; a `text_muted` ring replaces the mark.
            let muted = p.color(ColorRole::TextMuted);
            let ring = style::centered(mark, Size::new(size::ICON_SMALL, size::ICON_SMALL));
            p.spinner(ring, cx.spin, muted);
        }
        if self.wide {
            self.paint_texts(p, cx, inner, control);
        } else {
            let color = p.color(ColorRole::Text);
            let spec = style::CONTROL_TEXT.weight(Weight::Medium);
            style::draw_label(p, cx, &self.label, spec, inner, TextAlign::Start, color);
        }
        style::paint_error(p, cx, error, rect, row.h);
        if v.focused {
            style::focus_ring(p, row, Radius::Control);
        }
        p.set_opacity(previous);
    }

    /// Height of the row part of a wide check row (its error line follows).
    fn row_height(p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, error: Option<&Label>) -> f32 {
        let Some(error) = error else {
            return rect.h;
        };
        let scale = p.env().scale;
        let text = error.resolve(cx.tr);
        let h = p.layout(&text, style::SMALL_TEXT, Some(rect.w)).height / scale;
        (rect.h - h - Space::S4.points()).max(0.0)
    }

    /// Icon, label and second line of a wide row inside `inner` (after the box).
    fn paint_texts(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, mut inner: Rect, control: f32) {
        let direction = p.env().direction;
        if let Some(icon) = self.icon {
            let slot = style::take_start(&mut inner, size::ICON, direction);
            style::take_start(&mut inner, GAP.points(), direction);
            let muted = p.color(ColorRole::TextMuted);
            let at = style::centered(slot, Size::new(size::ICON, size::ICON));
            p.icon(icon, at, size::ICON, QuarterTurns::None, muted);
        }
        if control > 0.0 {
            style::take_end(&mut inner, control + GAP.points(), direction);
        }
        let scale = p.env().scale;
        let spec = style::CONTROL_TEXT
            .weight(Weight::Medium)
            .overflow(crate::text::Overflow::Wrap);
        let label_text = self.label.resolve(cx.tr);
        let label = p.layout(&label_text, spec, Some(inner.w));
        let label_h = label.height / scale;
        let sub = self.sublabel.as_ref().map(|s| {
            let text = s.resolve(cx.tr);
            p.layout(&text, style::SMALL_TEXT, Some(inner.w))
        });
        let sub_h = sub.as_ref().map_or(0.0, |l| l.height / scale);
        let top = inner.y + (inner.h - label_h - sub_h) / 2.0;
        let color = p.color(ColorRole::Text);
        p.text(
            &label,
            Rect::new(inner.x, top, inner.w, label_h),
            TextAlign::Start,
            color,
        );
        if let Some(sub) = sub {
            let muted = p.color(ColorRole::TextMuted);
            let at = Rect::new(inner.x, top + label_h, inner.w, sub_h);
            p.text(&sub, at, TextAlign::Start, muted);
        }
    }

    /// The box: accent fill with an `accent_text` edge and a dark mark when checked or
    /// mixed; a `control_border` outline when unchecked (red with an error).
    fn paint_box(&self, p: &mut Painter<'_>, mark: Rect, v: &Visual) {
        let border = p.hairline();
        if self.checked || self.mixed {
            let on = p.color(ColorRole::OnAccent);
            let fill = style::filled(p.color(ColorRole::Accent), on, v);
            p.fill(mark, Some(Radius::Control), fill);
            let edge = p.color(if v.error {
                ColorRole::Danger
            } else {
                ColorRole::AccentText
            });
            p.stroke(mark, Some(Radius::Control), border, edge);
            if v.loading {
                return;
            }
            if self.mixed {
                let middle = mark.center();
                let half = Space::S8.points() / 2.0 + Space::S4.points() / 2.0;
                let (from, to) = (
                    Point::new(middle.x - half, middle.y),
                    Point::new(middle.x + half, middle.y),
                );
                p.line(from, to, size::STROKE, on);
            } else {
                p.icon(
                    IconId::Check,
                    mark,
                    size::ICON_SMALL,
                    QuarterTurns::None,
                    on,
                );
            }
        } else {
            let bg = p.color(style::neutral_background(v));
            p.fill(mark, Some(Radius::Control), bg);
            let edge = p.color(if v.error {
                ColorRole::Danger
            } else {
                ColorRole::ControlBorder
            });
            p.stroke(mark, Some(Radius::Control), border, edge);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mixed_box_becomes_checked() {
        let mut mixed = Checkbox::new(Label::key("c"), false).mixed();
        assert!(mixed.toggle() && mixed.checked && !mixed.mixed);
        assert!(!mixed.toggle());
        let row = Checkbox::row(Rect::new(0.0, 0.0, 100.0, 80.0), 44.0);
        assert_eq!(row.h, ROW);
    }
}
