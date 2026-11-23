//! Select: one of several options. Closed it shows the chosen option; opened it shows the
//! options list as a floating popup.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, InputMode, Radius, Space};

/// Most options visible at once; longer lists scroll.
pub const MAX_VISIBLE_OPTIONS: usize = 6;

/// A select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Select {
    /// Accessible name (catalog key).
    pub name: Label,
    /// The options.
    pub options: Vec<Label>,
    /// Index of the chosen option.
    pub selected: usize,
}

const PAD: Space = Space::S12;
const CHEVRON: f32 = size::ICON_SMALL;
const CHEVRON_GAP: Space = Space::S8;
const POPUP_PAD: Space = Space::S4;

/// Where the options popup is and which options it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PopupView {
    pub(crate) rect: Rect,
    pub(crate) first: usize,
    pub(crate) count: usize,
    pub(crate) row_height: f32,
}

impl PopupView {
    /// The rectangle of the visible option `index`.
    pub(crate) fn row(&self, index: usize) -> Option<Rect> {
        let slot = index
            .checked_sub(self.first)
            .filter(|slot| *slot < self.count)?;
        let pad = POPUP_PAD.points();
        Some(Rect::new(
            self.rect.x + pad,
            self.rect.y + pad + slot as f32 * self.row_height,
            self.rect.w - 2.0 * pad,
            self.row_height,
        ))
    }

    /// The option under `pos`.
    pub(crate) fn option_at(&self, pos: Point) -> Option<usize> {
        (self.first..self.first + self.count)
            .find(|i| self.row(*i).is_some_and(|r| r.contains(pos)))
    }
}

impl Select {
    /// A select over `options` with the first one chosen.
    pub fn new(name: Label, options: Vec<Label>) -> Select {
        Select {
            name,
            options,
            selected: 0,
        }
    }

    /// The same select with option `index` chosen (ignored when out of range).
    pub fn selected(mut self, index: usize) -> Select {
        if index < self.options.len() {
            self.selected = index;
        }
        self
    }

    /// The field part of the node (the error line sits below it).
    pub(crate) fn field(rect: Rect, mode: InputMode) -> Rect {
        Rect::new(rect.x, rect.y, rect.w, mode.min_target().min(rect.h))
    }

    pub(crate) fn measure(
        &self,
        cx: &mut MeasureCx<'_>,
        avail: f32,
        error: Option<&Label>,
    ) -> Size {
        let mut widest = 0.0f32;
        for option in &self.options {
            widest = widest.max(cx.text_size(option, style::CONTROL_TEXT, None).w);
        }
        let natural = widest + 2.0 * PAD.points() + CHEVRON_GAP.points() + CHEVRON;
        let width = natural.min(avail).max(cx.mode.min_target());
        let height = cx.mode.min_target() + style::error_height(cx, error, width);
        Size::new(width, height)
    }

    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        error: Option<&Label>,
    ) {
        let previous = p.set_opacity(v.opacity());
        let field = Select::field(rect, cx.mode);
        let bg = p.color(style::neutral_background(v));
        p.fill(field, Some(Radius::Control), bg);
        let border = if v.error {
            ColorRole::Danger
        } else {
            ColorRole::ControlBorder
        };
        let (border, hairline) = (p.color(border), p.hairline());
        p.stroke(field, Some(Radius::Control), hairline, border);
        let direction = p.env().direction;
        let mut inner = style::inset_inline(field, PAD);
        let chevron = style::take_end(&mut inner, CHEVRON, direction);
        style::take_end(&mut inner, CHEVRON_GAP.points(), direction);
        let muted = p.color(ColorRole::TextMuted);
        if v.loading {
            p.spinner(
                style::centered(chevron, Size::new(CHEVRON, CHEVRON)),
                cx.spin,
                muted,
            );
        } else {
            p.icon(
                IconId::ChevronDown,
                chevron,
                CHEVRON,
                QuarterTurns::None,
                muted,
            );
        }
        if let Some(option) = self.options.get(self.selected) {
            let color = p.color(ColorRole::Text);
            style::draw_label(
                p,
                cx,
                option,
                style::CONTROL_TEXT,
                inner,
                TextAlign::Start,
                color,
            );
        }
        style::paint_error(p, cx, error, rect, field.h);
        if v.focused {
            style::focus_ring(p, field, Radius::Card);
        }
        p.set_opacity(previous);
    }

    /// Geometry of the popup for a field at `field` on a surface of `surface` points:
    /// below the field when it fits, otherwise above when that fits. When it fits on
    /// neither side it goes where more rows fit and shows only those (the list scrolls),
    /// so every visible option stays inside the surface.
    pub(crate) fn popup(
        &self,
        field: Rect,
        surface: Size,
        mode: InputMode,
        scroll: usize,
    ) -> PopupView {
        let all = self.options.len().min(MAX_VISIBLE_OPTIONS);
        let row_height = mode.min_target();
        let (pad, gap) = (POPUP_PAD.points(), Space::S4.points());
        let rows_in = |space: f32| {
            let rows = ((space - 2.0 * pad) / row_height).floor();
            (rows.max(0.0) as usize).min(all)
        };
        let below = rows_in(surface.h - field.bottom() - gap);
        let above = rows_in(field.y - gap);
        let (count, under) = if below >= all || below >= above {
            (below, true)
        } else {
            (above, false)
        };
        // At least one row, even on a surface too small for it.
        let count = count.max(all.min(1));
        let height = count as f32 * row_height + 2.0 * pad;
        let y = if under {
            field.bottom() + gap
        } else {
            field.y - gap - height
        };
        PopupView {
            rect: Rect::new(field.x, y, field.w, height),
            first: scroll.min(self.options.len() - count),
            count,
            row_height,
        }
    }

    pub(crate) fn paint_popup(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        view: &PopupView,
        highlight: usize,
    ) {
        p.shadow(view.rect, Some(Radius::Control));
        let (bg, line, hairline) = (
            p.color(ColorRole::Raised),
            p.color(ColorRole::Line),
            p.hairline(),
        );
        p.fill(view.rect, Some(Radius::Control), bg);
        p.stroke(view.rect, Some(Radius::Control), hairline, line);
        let direction = p.env().direction;
        for index in view.first..view.first + view.count {
            let (Some(row), Some(option)) = (view.row(index), self.options.get(index)) else {
                continue;
            };
            if index == highlight {
                let hover = p.color(ColorRole::Hover);
                p.fill(row, Some(Radius::Control), hover);
            }
            // The check mark sits in a slot at the start edge, as in menus (boards: Menus).
            let mut inner = style::inset_inline(row, Space::S8);
            let mark = style::take_start(&mut inner, CHEVRON, direction);
            style::take_start(&mut inner, CHEVRON_GAP.points(), direction);
            let chosen = index == self.selected;
            if chosen {
                let accent = p.color(ColorRole::AccentText);
                p.icon(IconId::Check, mark, CHEVRON, QuarterTurns::None, accent);
            }
            let weight = if chosen {
                Weight::Semibold
            } else {
                Weight::Regular
            };
            let color = p.color(ColorRole::Text);
            style::draw_label(
                p,
                cx,
                option,
                style::CONTROL_TEXT.weight(weight),
                inner,
                TextAlign::Start,
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn select(n: usize) -> Select {
        Select::new(
            Label::key("s"),
            (0..n).map(|i| Label::data(&i.to_string())).collect(),
        )
    }

    #[test]
    fn popup_goes_below_or_above_and_scrolls() {
        let mode = InputMode::Pointer;
        let target = mode.min_target();
        let surface = Size::new(10.0 * target, 10.0 * target);
        let top_field = Rect::new(0.0, 0.0, 4.0 * target, target);
        let view = select(3).popup(top_field, surface, mode, 0);
        assert!(view.rect.y > top_field.bottom());
        assert_eq!((view.first, view.count), (0, 3));
        let bottom_field = Rect::new(0.0, 9.0 * target, 4.0 * target, target);
        let above = select(3).popup(bottom_field, surface, mode, 0);
        assert!(above.rect.bottom() < bottom_field.y);
        let long = select(20).popup(top_field, surface, mode, 100);
        assert_eq!((long.first, long.count), (14, MAX_VISIBLE_OPTIONS));
        let row = long.row(15).unwrap();
        assert_eq!(long.option_at(row.center()), Some(15));
        assert_eq!(long.row(13), None);
        assert_eq!(long.row(20), None);
        assert_eq!(long.option_at(Point::new(-1.0, -1.0)), None);
        assert_eq!(select(0).popup(top_field, surface, mode, 0).count, 0);
        // Too low for six rows on either side: the side with more room, fewer rows.
        let low = Size::new(10.0 * target, 5.0 * target);
        let mid_field = Rect::new(0.0, target, 4.0 * target, target);
        let squeezed = select(6).popup(mid_field, low, mode, 0);
        assert_eq!((squeezed.first, squeezed.count), (0, 2));
        assert!(squeezed.rect.y > mid_field.bottom() && squeezed.rect.bottom() <= low.h);
        let scrolled = select(6).popup(mid_field, low, mode, 100);
        assert_eq!((scrolled.first, scrolled.count), (4, 2));
        let lower_field = Rect::new(0.0, 3.5 * target, 4.0 * target, target);
        let up = select(6).popup(lower_field, low, mode, 0);
        assert_eq!(up.count, 3);
        assert!(up.rect.y >= 0.0 && up.rect.bottom() < lower_field.y);
        let tiny = select(6).popup(top_field, Size::new(target, target), mode, 0);
        assert_eq!(tiny.count, 1, "never less than one row");
    }

    #[test]
    fn selection_builder_ignores_out_of_range() {
        assert_eq!(select(3).selected(2).selected, 2);
        assert_eq!(select(3).selected(9).selected, 0);
        let rect = Rect::new(0.0, 0.0, 100.0, 80.0);
        assert_eq!(
            Select::field(rect, InputMode::Touch).h,
            InputMode::Touch.min_target()
        );
    }
}
