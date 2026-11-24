//! Arrangement: items placed freely on a canvas that the user drags into place, with edges
//! snapping to the other items (boards "SettingsDisplay": the display arrangement, 144 high,
//! radius 8, `ground` with a `line` border; physical left-to-right in every language).
//! It is one focus stop like a radio group: a click or the arrow keys choose an item.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::{Painter, TextSpec};
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, Radius, Space, TypeSize};

/// Most items on one canvas.
pub const MAX_ARRANGEMENT_ITEMS: usize = 16;
/// Height of the canvas (boards: 144).
const HEIGHT: f32 = 3.0 * Space::S48.points();
/// Edges closer than this snap together.
const SNAP: f32 = Space::S12.points();

/// One item: where it is (points from the canvas' top-left corner) and what it shows.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrangedItem {
    /// Accessible name ("Display 1: Built-in 15.6″ (main)").
    pub name: Label,
    /// The big number or short title in the middle.
    pub title: Label,
    /// The small line under it ("Dell 24″").
    pub caption: Option<Label>,
    /// A small line above it ("main").
    pub tag: Option<Label>,
    /// Place and size on the canvas, physical left-to-right.
    pub rect: Rect,
}

impl ArrangedItem {
    /// An item at `rect` with its name and title.
    pub fn new(name: Label, title: Label, rect: Rect) -> ArrangedItem {
        ArrangedItem {
            name,
            title,
            caption: None,
            tag: None,
            rect,
        }
    }

    /// The same item with a caption under the title.
    pub fn caption(mut self, caption: Label) -> ArrangedItem {
        self.caption = Some(caption);
        self
    }

    /// The same item with a tag above the title.
    pub fn tag(mut self, tag: Label) -> ArrangedItem {
        self.tag = Some(tag);
        self
    }
}

/// The canvas and its items (at most [`MAX_ARRANGEMENT_ITEMS`]; more are dropped).
#[derive(Debug, Clone, PartialEq)]
pub struct Arrangement {
    /// Accessible name of the canvas.
    pub name: Label,
    /// The items.
    pub items: Vec<ArrangedItem>,
    /// The chosen item.
    pub selected: Option<usize>,
    /// The item being dragged and where it was grabbed (offset from its corner).
    pub(crate) grab: Option<(usize, Point, bool)>,
}

impl Arrangement {
    /// A canvas with `items`.
    pub fn new(name: Label, mut items: Vec<ArrangedItem>) -> Arrangement {
        items.truncate(MAX_ARRANGEMENT_ITEMS);
        Arrangement {
            name,
            items,
            selected: None,
            grab: None,
        }
    }

    /// The same canvas with item `index` chosen (ignored when out of range).
    pub fn selected(mut self, index: usize) -> Arrangement {
        if index < self.items.len() {
            self.selected = Some(index);
        }
        self
    }

    /// The item the keyboard acts on.
    pub(crate) fn current(&self) -> usize {
        self.selected.unwrap_or(0)
    }

    /// Item `index` in surface coordinates for a canvas at `rect`.
    pub(crate) fn item_rect(&self, rect: Rect, index: usize) -> Option<Rect> {
        let r = self.items.get(index)?.rect;
        Some(Rect::new(rect.x + r.x, rect.y + r.y, r.w, r.h))
    }

    /// The topmost item under `pos`.
    pub(crate) fn part_at(&self, rect: Rect, pos: Point) -> Option<usize> {
        (0..self.items.len())
            .rev()
            .find(|i| self.item_rect(rect, *i).is_some_and(|r| r.contains(pos)))
    }

    /// Moves item `index` so that its corner is at `corner` (canvas points of a canvas
    /// `width` wide): kept inside the canvas, edges within 12 points of another item's edge
    /// snap to it. Returns `true` when it moved.
    pub(crate) fn place(&mut self, index: usize, corner: Point, width: f32) -> bool {
        let Some(item) = self.items.get(index).map(|i| i.rect) else {
            return false;
        };
        let mut x = corner.x.clamp(0.0, (width - item.w).max(0.0));
        let mut y = corner.y.clamp(0.0, (HEIGHT - item.h).max(0.0));
        let others: Vec<Rect> = (0..self.items.len())
            .filter(|i| *i != index)
            .map(|i| self.items[i].rect)
            .collect();
        x = snap(x, item.w, others.iter().map(|o| (o.x, o.right())));
        y = snap(y, item.h, others.iter().map(|o| (o.y, o.bottom())));
        let moved = (x, y) != (item.x, item.y);
        if let Some(target) = self.items.get_mut(index) {
            target.rect.x = x;
            target.rect.y = y;
        }
        moved
    }

    pub(crate) fn measure(&self, _cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        Size::new(avail, HEIGHT)
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        let previous = p.set_opacity(v.opacity());
        let (ground, line) = (p.color(ColorRole::Ground), p.color(ColorRole::Line));
        p.fill(rect, Some(Radius::Control), ground);
        p.stroke(rect, Some(Radius::Control), p.hairline(), line);
        let saved = p.clip(rect);
        for index in 0..self.items.len() {
            if let Some(item) = self.item_rect(rect, index) {
                self.paint_item(p, cx, item, index);
            }
        }
        p.restore_clip(saved);
        if v.focused {
            if let Some(item) = self.item_rect(rect, self.current()) {
                style::focus_ring(p, item, Radius::Card);
            }
        }
        p.set_opacity(previous);
    }

    fn paint_item(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, index: usize) {
        let item = &self.items[index];
        let chosen = self.selected == Some(index);
        let (fill, edge, width) = if chosen {
            (ColorRole::Hover, ColorRole::AccentText, 2.0 * p.hairline())
        } else {
            (ColorRole::Raised, ColorRole::ControlBorder, p.hairline())
        };
        let (fill, edge) = (p.color(fill), p.color(edge));
        p.fill(rect, Some(Radius::Control), fill);
        p.stroke(rect, Some(Radius::Control), width, edge);
        let small = TextSpec::new(TypeSize::S12);
        let title_spec = TextSpec::title(TypeSize::S24);
        let lines: Vec<(&Label, TextSpec, ColorRole)> = [
            item.tag
                .as_ref()
                .map(|t| (t, small.weight(Weight::Semibold), ColorRole::Text)),
            Some((&item.title, title_spec, ColorRole::Text)),
            item.caption
                .as_ref()
                .map(|c| (c, small, ColorRole::TextMuted)),
        ]
        .into_iter()
        .flatten()
        .collect();
        let scale = p.env().scale;
        let layouts: Vec<_> = lines
            .iter()
            .map(|(label, spec, _)| p.layout(&label.resolve(cx.tr), *spec, Some(rect.w)))
            .collect();
        let total: f32 = layouts.iter().map(|l| l.height / scale).sum();
        let mut top = rect.y + (rect.h - total) / 2.0;
        for ((label, _, role), layout) in lines.iter().zip(&layouts) {
            let h = layout.height / scale;
            let color = p.color(*role);
            let mut row = Rect::new(rect.x, top, rect.w, h);
            if chosen && std::ptr::eq(*label, &item.title) {
                // Boards: a check mark (16) and 4 points before the title of the chosen item.
                let w = layout.width / scale + size::ICON_SMALL + Space::S4.points();
                row = style::centered(row, Size::new(w.min(rect.w), h));
                let mark = Rect::new(row.x, row.y, size::ICON_SMALL, h);
                p.icon(
                    IconId::Check,
                    mark,
                    size::ICON_SMALL,
                    QuarterTurns::None,
                    color,
                );
                row.x += size::ICON_SMALL + Space::S4.points();
                row.w -= size::ICON_SMALL + Space::S4.points();
                p.text(layout, row, TextAlign::Start, color);
            } else {
                p.text(layout, row, TextAlign::Center, color);
            }
            top += h;
        }
    }
}

/// `start` of a span of `length`, moved to the nearest place within [`SNAP`] where it
/// touches or lines up with one of the `spans` (start, end) of the other items: edge to
/// edge, start to start, end to end, or centre to centre.
fn snap(start: f32, length: f32, spans: impl Iterator<Item = (f32, f32)>) -> f32 {
    let mut best: Option<(f32, f32)> = None;
    for (low, high) in spans {
        let middle = (low + high - length) / 2.0;
        for candidate in [low, high, low - length, high - length, middle] {
            let distance = (candidate - start).abs();
            if distance <= SNAP && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, candidate));
            }
        }
    }
    best.map_or(start, |(_, at)| at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Arrangement {
        let item = |n: &str, x: f32| {
            ArrangedItem::new(
                Label::data(n),
                Label::data(n),
                Rect::new(x, 24.0, 144.0, 96.0),
            )
        };
        Arrangement::new(Label::key("a"), vec![item("1", 80.0), item("2", 400.0)])
    }

    #[test]
    fn items_snap_to_their_neighbours_and_stay_inside() {
        let mut a = canvas().selected(1).selected(7);
        assert_eq!(a.selected, Some(1));
        // 6 points from the right edge of item 1 (224): snaps to touch it.
        assert!(a.place(1, Point::new(230.0, 30.0), 640.0));
        assert_eq!((a.items[1].rect.x, a.items[1].rect.y), (224.0, 24.0));
        // Far away: no snapping, but clamped to the canvas.
        a.place(1, Point::new(9_000.0, -50.0), 640.0);
        assert_eq!((a.items[1].rect.x, a.items[1].rect.y), (496.0, 0.0));
        assert!(!a.place(9, Point::default(), 640.0));
        let rect = Rect::new(10.0, 10.0, 640.0, HEIGHT);
        assert_eq!(a.part_at(rect, Point::new(100.0, 60.0)), Some(0));
        assert_eq!(a.part_at(rect, Point::new(5.0, 5.0)), None);
    }
}
