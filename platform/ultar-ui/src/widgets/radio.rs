//! Radio group: exactly one of a few options, all visible – as a list of rows, as cards
//! (boards: installer scope, disk use, encryption) or as tiles (display mode).

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::paint::Painter;
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, state, ColorRole, InputMode, Radius, Space, TypeSize};
use ultar_i18n::Direction;

#[path = "radio_cards.rs"]
mod cards;

/// How the options of a radio group are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RadioLayout {
    /// One row per option, one below the other.
    #[default]
    List,
    /// One card per option (dot, title, detail), one below the other, full width.
    Cards,
    /// One card per option side by side, equal widths.
    CardRow,
    /// Tiles side by side, equal widths: centred title (with a check mark when chosen) and
    /// detail, no dot.
    Tiles,
}

/// A group of radio options. The group is one focus stop; the arrow keys move the choice
/// and skip options that are not available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioGroup {
    /// Accessible name of the group (catalog key).
    pub name: Label,
    /// The options.
    pub options: Vec<Label>,
    /// Index of the chosen option; `None` while nothing is chosen yet.
    pub selected: Option<usize>,
    /// Layout of the options.
    pub layout: RadioLayout,
    /// Second line per option (cards and tiles), by index.
    pub details: Vec<Option<Label>>,
    /// Options that cannot be chosen, with the reason (shown instead of the detail, also
    /// announced), by index.
    pub unavailable: Vec<Option<Label>>,
    /// Height of each card in 1/64 points (measured; empty before the first layout).
    pub(crate) heights: Vec<u32>,
    /// A bar of proportional parts in a card, by index (see [`RadioGroup::split`]).
    pub splits: Vec<Option<Vec<(Label, u32)>>>,
    /// A two-screen pictogram above the title of a tile, by index.
    pub pictograms: Vec<Option<Pictogram>>,
}

/// The two-screen pictogram of the display-mode tiles (boards: `data-illus`, 48 × 24, a
/// physical picture: never mirrored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pictogram {
    /// Two outlined screens joined by a line (one desktop across both).
    Extend,
    /// Two filled screens (the same picture on both).
    Mirror,
    /// The left screen filled, the right one dashed (switched off).
    First,
    /// The left screen dashed, the right one filled.
    Second,
}

const DOT: f32 = size::BOX;
/// Width of the ring of a chosen option (boards: 2).
const RING: f32 = size::FOCUS_RING;
const GAP: Space = Space::S12;
/// Fixed-point unit of [`RadioGroup::heights`].
const UNIT: f32 = 64.0;

impl RadioGroup {
    /// A group with nothing chosen, laid out as a list.
    pub fn new(name: Label, options: Vec<Label>) -> RadioGroup {
        RadioGroup {
            name,
            options,
            selected: None,
            layout: RadioLayout::List,
            details: Vec::new(),
            unavailable: Vec::new(),
            heights: Vec::new(),
            splits: Vec::new(),
            pictograms: Vec::new(),
        }
    }

    /// The same group with a two-screen pictogram above the title of tile `index`
    /// ([`RadioLayout::Tiles`] only).
    pub fn pictogram(mut self, index: usize, picture: Pictogram) -> RadioGroup {
        if index < self.options.len() {
            if self.pictograms.len() <= index {
                self.pictograms.resize(index + 1, None);
            }
            self.pictograms[index] = Some(picture);
        }
        self
    }

    /// The same group with a bar of proportional parts under the second line of card
    /// `index` (boards: the disk split of "beside the existing system"): each part with its
    /// label and weight; the last part is the new one (hover fill, two-point `accent_text`
    /// edge at its start). Cards layouts only; at most 8 parts.
    pub fn split(mut self, index: usize, parts: Vec<(Label, u32)>) -> RadioGroup {
        if index < self.options.len() && !parts.is_empty() && parts.len() <= 8 {
            if self.splits.len() <= index {
                self.splits.resize(index + 1, None);
            }
            self.splits[index] = Some(parts);
        }
        self
    }

    /// The same group with option `index` chosen (ignored when out of range or unavailable).
    pub fn selected(mut self, index: usize) -> RadioGroup {
        if index < self.options.len() && !self.is_unavailable(index) {
            self.selected = Some(index);
        }
        self
    }

    /// The same group in another layout.
    pub fn layout(mut self, layout: RadioLayout) -> RadioGroup {
        self.layout = layout;
        self
    }

    /// The same group with a second line under option `index` (cards and tiles).
    pub fn detail(mut self, index: usize, detail: Label) -> RadioGroup {
        set_at(&mut self.details, index, detail, self.options.len());
        self
    }

    /// The same group with option `index` not available, for `reason` (boards: "not
    /// available on this device: the TPM chip is off"). A chosen option stays chosen only
    /// if it is still available.
    pub fn unavailable(mut self, index: usize, reason: Label) -> RadioGroup {
        set_at(&mut self.unavailable, index, reason, self.options.len());
        if self.selected == Some(index) {
            self.selected = None;
        }
        self
    }

    /// `true` when option `index` cannot be chosen.
    pub fn is_unavailable(&self, index: usize) -> bool {
        self.unavailable.get(index).is_some_and(Option::is_some)
    }

    /// The reason option `index` cannot be chosen.
    pub fn reason(&self, index: usize) -> Option<&Label> {
        self.unavailable.get(index).and_then(Option::as_ref)
    }

    /// The option the keyboard acts on: the chosen one, or the first available one.
    pub(crate) fn current(&self) -> usize {
        self.selected
            .or_else(|| (0..self.options.len()).find(|i| !self.is_unavailable(*i)))
            .unwrap_or(0)
    }

    /// The next available option from `from` (wrapping); `from` itself when none is.
    pub(crate) fn step(&self, from: usize, forward: bool) -> usize {
        let count = self.options.len();
        let mut at = from;
        for _ in 0..count {
            at = if forward {
                (at + 1) % count
            } else {
                (at + count - 1) % count
            };
            if !self.is_unavailable(at) {
                return at;
            }
        }
        from
    }

    fn height_of(&self, index: usize, mode: InputMode) -> f32 {
        match self.heights.get(index) {
            Some(h) if self.layout != RadioLayout::List => *h as f32 / UNIT,
            _ => mode.min_target(),
        }
    }

    /// The rectangle of option `index`: its row, card or tile.
    pub(crate) fn row(&self, rect: Rect, index: usize, mode: InputMode, dir: Direction) -> Rect {
        let gap = if self.layout == RadioLayout::List {
            0.0
        } else {
            GAP.points()
        };
        match self.layout {
            RadioLayout::List | RadioLayout::Cards => {
                let top: f32 = (0..index).map(|i| self.height_of(i, mode) + gap).sum();
                Rect::new(rect.x, rect.y + top, rect.w, self.height_of(index, mode))
            }
            RadioLayout::CardRow | RadioLayout::Tiles => {
                let count = self.options.len().max(1) as f32;
                let width = ((rect.w - gap * (count - 1.0)) / count).max(0.0);
                let offset = index as f32 * (width + gap);
                let x = if dir.is_rtl() {
                    rect.right() - offset - width
                } else {
                    rect.x + offset
                };
                Rect::new(x, rect.y, width, rect.h)
            }
        }
    }

    /// The option under `pos`.
    pub(crate) fn part_at(
        &self,
        rect: Rect,
        pos: Point,
        mode: InputMode,
        dir: Direction,
    ) -> Option<usize> {
        (0..self.options.len()).find(|i| self.row(rect, *i, mode, dir).contains(pos))
    }

    /// Size of the group; cards and tiles keep their heights for placing them.
    pub(crate) fn measure(&mut self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        if self.layout != RadioLayout::List {
            return self.measure_cards(cx, avail);
        }
        let mut widest = 0.0f32;
        for option in &self.options {
            widest = widest.max(cx.text_size(option, style::CONTROL_TEXT, None).w);
        }
        let natural = DOT + GAP.points() + widest;
        let rows = self.options.len().max(1) as f32;
        Size::new(natural.min(avail).max(DOT), rows * cx.mode.min_target())
    }

    /// Paints the group; `parts` are the options under the pointer (hovered, pressed).
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
        for index in 0..self.options.len() {
            let row = self.row(rect, index, cx.mode, direction);
            let local = Visual {
                hovered: v.hovered && parts.0 == Some(index),
                pressed: v.pressed && parts.1 == Some(index),
                focused: v.focused && index == self.current(),
                ..*v
            };
            if self.layout == RadioLayout::List {
                self.paint_row(p, cx, row, index, &local);
            } else {
                self.paint_card(p, cx, row, index, &local);
            }
        }
        p.set_opacity(previous);
    }

    fn paint_row(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        row: Rect,
        index: usize,
        v: &Visual,
    ) {
        let direction = p.env().direction;
        let mut inner = row;
        let slot = style::take_start(&mut inner, DOT, direction);
        style::take_start(&mut inner, GAP.points(), direction);
        let dot = style::centered(slot, Size::new(DOT, DOT));
        let unavailable = self.is_unavailable(index);
        self.paint_dot(p, dot, v, (self.selected == Some(index), unavailable));
        let color = p.color(if unavailable {
            ColorRole::ControlBorder
        } else {
            ColorRole::Text
        });
        let label = &self.options[index];
        style::draw_label(
            p,
            cx,
            label,
            style::CONTROL_TEXT,
            inner,
            TextAlign::Start,
            color,
        );
        if v.focused {
            // Inside the row: rows touch, a ring outside would cover the neighbours.
            let ring = p.color(state::FOCUS_RING);
            p.stroke(row, Some(Radius::Control), size::FOCUS_RING, ring);
        }
    }

    /// Boards: a 2-point `accent_text` ring with an 8-point `accent_text` dot when chosen,
    /// a `control_border` ring otherwise, a `line` ring when not available.
    fn paint_dot(&self, p: &mut Painter<'_>, dot: Rect, v: &Visual, (chosen, off): (bool, bool)) {
        if !off {
            let bg = p.color(style::neutral_background(v));
            p.fill(dot, Some(Radius::Round), bg);
        }
        if chosen {
            let accent = p.color(ColorRole::AccentText);
            p.stroke(dot, Some(Radius::Round), RING, accent);
            let inner = style::centered(dot, Size::new(Space::S8.points(), Space::S8.points()));
            p.fill(inner, Some(Radius::Round), accent);
        } else {
            let role = if off {
                ColorRole::Line
            } else {
                ColorRole::ControlBorder
            };
            let ring = p.color(role);
            p.stroke(dot, Some(Radius::Round), p.hairline(), ring);
        }
    }
}

/// Puts `value` at `index` of a sparse per-option list (ignored past `count`).
fn set_at(list: &mut Vec<Option<Label>>, index: usize, value: Label, count: usize) {
    if index >= count {
        return;
    }
    if list.len() <= index {
        list.resize(index + 1, None);
    }
    list[index] = Some(value);
}

/// Height of one line of `size` text (the language decides the line height).
fn line(cx: &MeasureCx<'_>, size: TypeSize) -> f32 {
    super::list::line_box(cx.env, size)
}

/// The title of a card or tile: body text, semibold.
const TITLE: crate::paint::TextSpec = style::CONTROL_TEXT.weight(Weight::Semibold);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_hit_row_by_row() {
        let options = (0..3).map(|i| Label::data(&i.to_string())).collect();
        let group = RadioGroup::new(Label::key("g"), options);
        assert_eq!((group.selected, group.current()), (None, 0));
        let group = group.selected(9).selected(2);
        assert_eq!(group.selected, Some(2));
        let mode = InputMode::Pointer;
        let h = mode.min_target();
        let rect = Rect::new(0.0, 0.0, 4.0 * h, 3.0 * h);
        let ltr = Direction::Ltr;
        assert_eq!(
            group.part_at(rect, Point::new(1.0, 0.0), mode, ltr),
            Some(0)
        );
        assert_eq!(
            group.part_at(rect, Point::new(1.0, 2.5 * h), mode, ltr),
            Some(2)
        );
        assert_eq!(
            group.part_at(rect, Point::new(1.0, 3.0 * h), mode, ltr),
            None
        );
        assert_eq!(group.part_at(rect, Point::new(-1.0, h), mode, ltr), None);
    }

    #[test]
    fn unavailable_options_are_skipped_and_never_chosen() {
        let options = (0..3).map(|i| Label::data(&i.to_string())).collect();
        let group = RadioGroup::new(Label::key("g"), options)
            .selected(1)
            .unavailable(1, Label::key("why"))
            .unavailable(7, Label::key("ignored"));
        assert_eq!(group.selected, None, "an unavailable choice is dropped");
        assert_eq!(group.current(), 0);
        assert_eq!(group.step(0, true), 2);
        assert_eq!(group.step(2, false), 0);
        assert_eq!(group.reason(1), Some(&Label::key("why")));
        assert_eq!(group.clone().selected(1).selected, None);
        assert_eq!(group.unavailable.len(), 2);
    }

    #[test]
    fn a_row_of_cards_runs_from_the_start_edge() {
        let options = (0..2).map(|i| Label::data(&i.to_string())).collect();
        let group = RadioGroup::new(Label::key("g"), options).layout(RadioLayout::CardRow);
        let rect = Rect::new(0.0, 0.0, 212.0, 60.0);
        let mode = InputMode::Pointer;
        assert_eq!(
            group.row(rect, 0, mode, Direction::Ltr),
            Rect::new(0.0, 0.0, 100.0, 60.0)
        );
        assert_eq!(
            group.row(rect, 0, mode, Direction::Rtl),
            Rect::new(112.0, 0.0, 100.0, 60.0)
        );
    }
}
