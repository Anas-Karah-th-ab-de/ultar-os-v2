//! Tabs: switch between views of the same place. The tab bar is one focus stop; the arrow
//! keys (mapped by direction) choose the neighbouring tab.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::paint::Painter;
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, Radius, Space};
use ultar_i18n::Direction;

/// A tab bar. Each tab is as wide as its label plus padding (boards "Bricks"); a bar that
/// does not fit gives every tab an equal share and scrolls by whole tabs.
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
    /// Natural width of each tab in 1/64 points, measured at the weight it is drawn with
    /// (the chosen tab semibold); empty before the first layout.
    pub(crate) widths: Vec<u32>,
    /// Filled pills instead of an underlined strip (boards PhoneDial "filter"): label + 2 × 12,
    /// 4 apart, the chosen pill on the `hover` fill, no rule and no underline.
    pub(crate) pills: bool,
}

/// Fixed-point unit of [`Tabs::widths`] (1/64 point keeps the type `Eq`).
const UNIT: f32 = 64.0;

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
            widths: Vec::new(),
            pills: false,
        }
    }

    /// The same bar drawn as filled pills (a filter of one list, boards PhoneDial).
    pub fn pills(mut self) -> Tabs {
        self.pills = true;
        self
    }

    /// Inline padding of a tab.
    fn pad(&self) -> f32 {
        if self.pills {
            Space::S12.points()
        } else {
            PAD.points()
        }
    }

    /// Space between two tabs.
    fn gap(&self) -> f32 {
        if self.pills {
            Space::S4.points()
        } else {
            0.0
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

    /// The natural widths in points when they are known and the bar has room for them.
    fn natural(&self, rect: Rect) -> Option<Vec<f32>> {
        if self.widths.len() != self.tabs.len() || self.tabs.is_empty() {
            return None;
        }
        let widths: Vec<f32> = self.widths.iter().map(|w| *w as f32 / UNIT).collect();
        let gaps = self.gap() * (widths.len() as f32 - 1.0);
        (widths.iter().sum::<f32>() + gaps <= rect.w + 0.5).then_some(widths)
    }

    /// How many whole tabs the bar shows at once.
    fn visible_count(&self, rect: Rect) -> usize {
        if self.natural(rect).is_some() {
            return self.tabs.len().max(1);
        }
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
        let (offset, width) = match self.natural(rect) {
            Some(widths) => (
                widths.iter().take(index).sum::<f32>() + self.gap() * index as f32,
                widths.get(index).copied().unwrap_or(0.0),
            ),
            None => {
                let width = self.tab_width(rect);
                ((index as f32 - self.first as f32) * width, width)
            }
        };
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

    /// Measures every tab (label at its drawn weight plus padding 16 on both sides) and
    /// keeps the widths for placing the tabs.
    pub(crate) fn measure(&mut self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        self.widths.clear();
        for (index, tab) in self.tabs.iter().enumerate() {
            let weight = if index == self.selected {
                Weight::Semibold
            } else {
                Weight::Regular
            };
            let text = cx.text_advance(tab, style::CONTROL_TEXT.weight(weight));
            let width = text + 2.0 * self.pad();
            self.widths.push((width * UNIT).round().max(0.0) as u32);
        }
        let count = self.tabs.len() as f32;
        let natural = self.widths.iter().map(|w| *w as f32 / UNIT).sum::<f32>()
            + self.gap() * (count - 1.0).max(0.0);
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
        if !self.pills {
            let rule = Rect::new(rect.x, rect.bottom() - hairline, rect.w, hairline);
            p.fill(rule, None, line);
        }
        // Tabs scrolled out of the strip are cut at its edges.
        let saved = p.clip(rect.inflate(style::focus_outset()));
        for (index, label) in self.tabs.iter().enumerate() {
            let tab = self.tab(rect, index, direction);
            let state = Visual {
                hovered: v.hovered && parts.0 == Some(index),
                pressed: v.pressed && parts.1 == Some(index),
                ..*v
            };
            let chosen = index == self.selected;
            if state.hovered || state.pressed {
                let bg = p.color(style::neutral_background(&state));
                p.fill(tab, Some(Radius::Control), bg);
            } else if chosen && self.pills {
                let bg = p.color(ColorRole::Hover);
                p.fill(tab, Some(Radius::Control), bg);
            }
            let (role, weight) = if chosen {
                (ColorRole::Text, Weight::Semibold)
            } else {
                (ColorRole::TextMuted, Weight::Regular)
            };
            let color = p.color(role);
            let spec = style::CONTROL_TEXT.weight(weight);
            let inner = style::inset_inline(tab, Space::S8);
            style::draw_label(p, cx, label, spec, inner, TextAlign::Center, color);
            if chosen && self.pills && v.focused {
                style::focus_ring(p, tab.inflate(-style::focus_outset()), Radius::Control);
            } else if chosen && !self.pills {
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
