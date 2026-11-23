//! Tree: items in levels that open and close (folders, settings areas). Items are data, so a
//! large tree costs one node; only visible items are laid out and painted.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, state, ColorRole, InputMode, Radius, Space};
use ultar_i18n::Direction;

/// Most items a tree holds; deepest level.
pub const MAX_TREE_ITEMS: usize = 100_000;
/// Deepest level of a tree item (0 = top level).
pub const MAX_TREE_DEPTH: u8 = 16;

/// One item of a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeItem {
    /// The label.
    pub label: Label,
    /// Icon before the label.
    pub icon: Option<IconId>,
    /// Level: 0 for the top, one more for each step down. An item followed by deeper items
    /// has children.
    pub depth: u8,
    /// `true` while its children are shown.
    pub expanded: bool,
}

impl TreeItem {
    /// An item at `depth`, closed.
    pub fn new(label: Label, depth: u8) -> TreeItem {
        TreeItem {
            label,
            icon: None,
            depth: depth.min(MAX_TREE_DEPTH),
            expanded: false,
        }
    }

    /// The same item with an icon.
    pub fn icon(mut self, icon: IconId) -> TreeItem {
        self.icon = Some(icon);
        self
    }

    /// The same item open.
    pub fn expanded(mut self) -> TreeItem {
        self.expanded = true;
        self
    }
}

/// A tree, flattened in reading order (an item's children follow it with a larger depth).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    /// Accessible name (catalog key).
    pub name: Label,
    /// The items.
    pub items: Vec<TreeItem>,
    /// The selected item (also where the keyboard is).
    pub selected: Option<usize>,
}

const INDENT: Space = Space::S16;
const TOGGLE: f32 = size::ICON_SMALL;
const GAP: Space = Space::S8;

impl Tree {
    /// A tree of `items` (beyond the limit dropped; a jump of more than one level is
    /// flattened to one level).
    pub fn new(name: Label, mut items: Vec<TreeItem>) -> Tree {
        items.truncate(MAX_TREE_ITEMS);
        let mut previous: Option<u8> = None;
        for item in &mut items {
            let limit = previous.map_or(0, |p| p.saturating_add(1));
            item.depth = item.depth.min(limit);
            previous = Some(item.depth);
        }
        Tree {
            name,
            items,
            selected: None,
        }
    }

    /// `true` when item `index` has children.
    pub fn has_children(&self, index: usize) -> bool {
        match (self.items.get(index), self.items.get(index + 1)) {
            (Some(item), Some(next)) => next.depth > item.depth,
            _ => false,
        }
    }

    /// The parent of item `index`.
    pub fn parent(&self, index: usize) -> Option<usize> {
        let depth = self.items.get(index)?.depth;
        (0..index).rev().find(|i| self.items[*i].depth < depth)
    }

    /// The items that are shown (all ancestors open), in order.
    pub fn visible(&self) -> Vec<usize> {
        let mut shown = Vec::with_capacity(self.items.len());
        let mut hide_below: Option<u8> = None;
        for (index, item) in self.items.iter().enumerate() {
            match hide_below {
                Some(depth) if item.depth > depth => continue,
                _ => hide_below = None,
            }
            shown.push(index);
            if !item.expanded {
                hide_below = Some(item.depth);
            }
        }
        shown
    }

    /// Position of item `index` among its siblings and their count (1-based).
    pub fn position(&self, index: usize) -> Option<(usize, usize)> {
        let depth = self.items.get(index)?.depth;
        let parent = self.parent(index);
        let start = parent.map_or(0, |p| p + 1);
        let siblings: Vec<usize> = (start..self.items.len())
            .take_while(|i| self.items[*i].depth >= depth)
            .filter(|i| self.items[*i].depth == depth)
            .collect();
        let at = siblings.iter().position(|i| *i == index)?;
        Some((at + 1, siblings.len()))
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let rows = self.visible().len().max(1) as f32;
        Size::new(avail, rows * cx.mode.min_target())
    }

    /// The row of the `slot`-th visible item.
    pub(crate) fn row(&self, rect: Rect, slot: usize, mode: InputMode) -> Rect {
        let h = mode.min_target();
        Rect::new(rect.x, rect.y + slot as f32 * h, rect.w, h)
    }

    /// The toggle (open/close) area of an item in its row.
    fn toggle(&self, row: Rect, index: usize, direction: Direction) -> Rect {
        let depth = self.items.get(index).map_or(0, |i| i.depth);
        let mut inner = row;
        style::take_start(&mut inner, f32::from(depth) * INDENT.points(), direction);
        style::take_start(&mut inner, row.h, direction)
    }

    /// The part under `pos`: item index × 2, plus 1 on its toggle.
    pub(crate) fn part_at(
        &self,
        rect: Rect,
        pos: Point,
        mode: InputMode,
        direction: Direction,
    ) -> Option<usize> {
        if !rect.contains(pos) {
            return None;
        }
        let slot = ((pos.y - rect.y) / mode.min_target()).floor() as usize;
        let index = *self.visible().get(slot)?;
        let on_toggle = self.has_children(index)
            && self
                .toggle(self.row(rect, slot, mode), index, direction)
                .contains(pos);
        Some(index * 2 + usize::from(on_toggle))
    }

    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        hover: Option<usize>,
    ) {
        let previous = p.set_opacity(v.opacity());
        let visible = p.visible().intersect(&rect);
        let h = cx.mode.min_target();
        let first = ((visible.y - rect.y) / h).floor().max(0.0) as usize;
        let last = ((visible.bottom() - rect.y) / h).ceil().max(0.0) as usize;
        for (slot, index) in self.visible().into_iter().enumerate() {
            if slot < first || slot > last {
                continue;
            }
            let row = self.row(rect, slot, cx.mode);
            let hovered = hover.is_some_and(|part| part / 2 == index);
            self.paint_item(p, cx, row, index, hovered);
            if v.focused && self.selected == Some(index) {
                let ring = p.color(state::FOCUS_RING);
                p.stroke(row, Some(Radius::Control), size::FOCUS_RING, ring);
            }
        }
        p.set_opacity(previous);
    }

    fn paint_item(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        row: Rect,
        index: usize,
        hovered: bool,
    ) {
        let Some(item) = self.items.get(index) else {
            return;
        };
        let direction = p.env().direction;
        let selected = self.selected == Some(index);
        if selected || hovered {
            let bg = p.color(if selected {
                ColorRole::Line
            } else {
                ColorRole::Hover
            });
            p.fill(row, Some(Radius::Control), bg);
        }
        let toggle = self.toggle(row, index, direction);
        // The label starts after the toggle, on the end side of it.
        let mut inner = if direction.is_rtl() {
            Rect::new(row.x, row.y, (toggle.x - row.x).max(0.0), row.h)
        } else {
            Rect::new(
                toggle.right(),
                row.y,
                (row.right() - toggle.right()).max(0.0),
                row.h,
            )
        };
        let muted = p.color(ColorRole::TextMuted);
        if self.has_children(index) {
            let turns = if item.expanded {
                QuarterTurns::One
            } else {
                QuarterTurns::None
            };
            p.icon(IconId::Forward, toggle, TOGGLE, turns, muted);
        }
        if let Some(icon) = item.icon {
            let slot = style::take_start(&mut inner, size::ICON_SMALL, direction);
            style::take_start(&mut inner, GAP.points(), direction);
            p.icon(icon, slot, size::ICON_SMALL, QuarterTurns::None, muted);
        }
        let weight = if selected {
            Weight::Semibold
        } else {
            Weight::Regular
        };
        let color = p.color(ColorRole::Text);
        let spec = style::CONTROL_TEXT.weight(weight);
        style::draw_label(p, cx, &item.label, spec, inner, TextAlign::Start, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Tree {
        let item = |d| TreeItem::new(Label::data(&format!("{d}")), d);
        Tree::new(
            Label::key("t"),
            vec![
                item(0).expanded(),
                item(1),
                item(1),
                item(2),
                item(0),
                item(5),
            ],
        )
    }

    #[test]
    fn structure_visibility_and_positions() {
        let t = tree();
        assert_eq!(t.items[5].depth, 1, "a jump of four levels is flattened");
        assert!(t.has_children(0) && !t.has_children(1) && t.has_children(2));
        assert_eq!(t.visible(), vec![0, 1, 2, 4]);
        assert_eq!(t.parent(3), Some(2));
        assert_eq!(t.parent(0), None);
        assert_eq!(t.position(2), Some((2, 2)));
        assert_eq!(t.position(4), Some((2, 2)));
        assert_eq!(t.position(9), None);
        let mode = InputMode::Pointer;
        let rect = Rect::new(0.0, 0.0, 200.0, 200.0);
        let h = mode.min_target();
        assert_eq!(
            t.part_at(rect, Point::new(150.0, 1.5 * h), mode, Direction::Ltr),
            Some(2)
        );
        let toggle = Point::new(h + 4.0, 2.5 * h);
        assert_eq!(
            t.part_at(rect, toggle, mode, Direction::Ltr),
            Some(5),
            "toggle of item 2"
        );
        assert_eq!(
            t.part_at(rect, Point::new(5.0, 9.0 * h), mode, Direction::Ltr),
            None
        );
    }
}
