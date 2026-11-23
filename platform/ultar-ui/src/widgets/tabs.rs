//! Tabs: switch between views of the same place. The tab bar is one focus stop; the arrow
//! keys (mapped by direction) choose the neighbouring tab.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::paint::Painter;
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, Radius, Space};
use ultar_i18n::Direction;

/// A tab bar. All tabs have the same width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tabs {
    /// Accessible name of the bar (catalog key).
    pub name: Label,
    /// The tab labels.
    pub tabs: Vec<Label>,
    /// Index of the shown tab.
    pub selected: usize,
    /// The first tab drawn at the start edge when the bar does not fit (the strip scrolls
    /// by whole tabs so that the chosen one is always visible).
    pub(crate) first: usize,
}

const PAD: Space = Space::S16;
const INDICATOR: f32 = size::FOCUS_RING;

impl Tabs {
    /// A tab bar showing the first tab.
    pub fn new(name: Label, tabs: Vec<Label>) -> Tabs {
        Tabs {
            name,
            tabs,
            selected: 0,
            first: 0,
        }
    }

    /// The same bar showing tab `index` (ignored when out of range).
    pub fn selected(mut self, index: usize) -> Tabs {
        if index < self.tabs.len() {
            self.selected = index;
        }
        self
    }

    /// Width of one tab: an equal share of the bar, never below the target size (the bar
    /// is one target high).
    fn tab_width(&self, rect: Rect) -> f32 {
        let width = (rect.w / self.tabs.len().max(1) as f32).max(rect.h);
        if width.is_finite() {
            width
        } else {
            0.0
        }
    }

    /// How many whole tabs the bar shows at once.
    fn visible_count(&self, rect: Rect) -> usize {
        let width = self.tab_width(rect);
        if width > 0.0 {
            ((rect.w / width).floor() as usize).max(1)
        } else {
            self.tabs.len()
        }
    }

    /// Scrolls the strip by whole tabs so that the chosen tab is fully inside `rect`, with
    /// its neighbour visible too when there is one (so that the next tab can be reached by
    /// tapping the last visible one). Returns `true` when the strip moved.
    pub(crate) fn keep_chosen_visible(&mut self, rect: Rect) -> bool {
        let shown = self.visible_count(rect);
        let last_first = self.tabs.len().saturating_sub(shown);
        let context = usize::from(shown >= 3);
        let mut first = self.first.min(last_first);
        let low = self.selected.saturating_sub(context);
        let high = (self.selected + context).min(self.tabs.len().saturating_sub(1));
        if low < first {
            first = low;
        } else if high >= first + shown {
            first = (high + 1).saturating_sub(shown).min(last_first);
        }
        let moved = first != self.first;
        self.first = first;
        moved
    }

    /// Keeps the strip inside its range (after a resize). Returns `true` when it moved.
    pub(crate) fn clamp_strip(&mut self, rect: Rect) -> bool {
        let last_first = self.tabs.len().saturating_sub(self.visible_count(rect));
        let moved = self.first > last_first;
        self.first = self.first.min(last_first);
        moved
    }

    /// Scrolls the strip by `steps` tabs towards the end (negative: the start), within its
    /// range. Returns `true` when it moved.
    pub(crate) fn scroll_strip(&mut self, rect: Rect, steps: i32) -> bool {
        let last_first = self.tabs.len().saturating_sub(self.visible_count(rect));
        let first = (self.first as i64 + i64::from(steps)).clamp(0, last_first as i64) as usize;
        let moved = first != self.first;
        self.first = first;
        moved
    }

    /// The rectangle of tab `index`: tabs follow each other from the start edge (shifted by
    /// the tabs scrolled out at the start).
    pub(crate) fn tab(&self, rect: Rect, index: usize, direction: Direction) -> Rect {
        let width = self.tab_width(rect);
        let offset = (index as f32 - self.first as f32) * width;
        let x = if direction.is_rtl() {
            rect.right() - offset - width
        } else {
            rect.x + offset
        };
        Rect::new(x, rect.y, width, rect.h)
    }

    /// The tab under `pos` (only inside the bar).
    pub(crate) fn part_at(&self, rect: Rect, pos: Point, direction: Direction) -> Option<usize> {
        if !rect.contains(pos) {
            return None;
        }
        (0..self.tabs.len()).find(|i| self.tab(rect, *i, direction).contains(pos))
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let spec = style::CONTROL_TEXT.weight(Weight::Semibold);
        let mut widest = 0.0f32;
        for tab in &self.tabs {
            widest = widest.max(cx.text_size(tab, spec, None).w);
        }
        let count = self.tabs.len() as f32;
        let natural = (widest + 2.0 * PAD.points()) * count;
        // Every tab stays a full target: a bar of many tabs on a narrow surface scrolls
        // (by whole tabs, the chosen one always visible) instead of shrinking them.
        let minimum = (cx.mode.min_target() * count).min(avail.max(cx.mode.min_target()));
        Size::new(natural.min(avail).max(minimum), cx.mode.min_target())
    }

    /// Paints the bar; `parts` are the tabs under the pointer (hovered, pressed).
    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        parts: (Option<usize>, Option<usize>),
    ) {
        let previous = p.set_opacity(v.opacity());
        let direction = p.env().direction;
        let (line, hairline) = (p.color(ColorRole::Line), p.hairline());
        let rule = Rect::new(rect.x, rect.bottom() - hairline, rect.w, hairline);
        p.fill(rule, None, line);
        // Tabs scrolled out of the strip are cut at its edges.
        let saved = p.clip(rect.inflate(style::focus_outset()));
        for (index, label) in self.tabs.iter().enumerate() {
            let tab = self.tab(rect, index, direction);
            let state = Visual {
                hovered: v.hovered && parts.0 == Some(index),
                pressed: v.pressed && parts.1 == Some(index),
                ..*v
            };
            if state.hovered || state.pressed {
                let bg = p.color(style::neutral_background(&state));
                p.fill(tab, Some(Radius::Control), bg);
            }
            let chosen = index == self.selected;
            let (role, weight) = if chosen {
                (ColorRole::Text, Weight::Semibold)
            } else {
                (ColorRole::TextMuted, Weight::Regular)
            };
            let color = p.color(role);
            let spec = style::CONTROL_TEXT.weight(weight);
            let inner = style::inset_inline(tab, Space::S8);
            style::draw_label(p, cx, label, spec, inner, TextAlign::Center, color);
            if chosen {
                let accent = p.color(ColorRole::AccentText);
                let bar = style::inset_inline(tab, Space::S8);
                let bar = Rect::new(bar.x, tab.bottom() - INDICATOR, bar.w, INDICATOR);
                p.fill(bar, Some(Radius::Round), accent);
                if v.focused {
                    style::focus_ring(p, tab.inflate(-style::focus_outset()), Radius::Control);
                }
            }
        }
        p.restore_clip(saved);
        p.set_opacity(previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_run_from_the_start_edge_in_both_directions() {
        let labels = (0..4).map(|i| Label::data(&i.to_string())).collect();
        let bar = Tabs::new(Label::key("t"), labels).selected(7).selected(1);
        assert_eq!(bar.selected, 1);
        let unit = Space::S48.points();
        let rect = Rect::new(unit, 0.0, 4.0 * unit, unit);
        assert_eq!(bar.tab(rect, 0, Direction::Ltr).x, unit);
        assert_eq!(bar.tab(rect, 0, Direction::Rtl).right(), 5.0 * unit);
        let near_left = Point::new(unit + 1.0, 1.0);
        assert_eq!(bar.part_at(rect, near_left, Direction::Ltr), Some(0));
        assert_eq!(bar.part_at(rect, near_left, Direction::Rtl), Some(3));
        assert_eq!(
            bar.part_at(rect, Point::new(0.0, 1.0), Direction::Ltr),
            None
        );
        let empty = Tabs::new(Label::key("t"), Vec::new());
        assert_eq!(empty.part_at(rect, near_left, Direction::Ltr), None);
        assert_eq!(empty.tab(rect, 0, Direction::Ltr).w, rect.w);
    }
}
