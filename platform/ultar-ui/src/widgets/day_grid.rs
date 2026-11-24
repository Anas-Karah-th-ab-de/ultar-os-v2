//! The month grid of the calendar sheet (`design/boards/b2.py`, CalendarSheet): weekday
//! headers and day cells in rows of seven, one Tab stop; arrows move the selected day,
//! a click selects one (`Selected(index)`). Columns follow the reading direction.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::paint::{Painter, TextSpec};
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, Radius, Space, TypeSize};
use ultar_i18n::Direction;

/// Most cells of a month grid (six weeks).
pub const MAX_DAY_CELLS: usize = 42;
/// Columns (days of a week).
pub const WEEK: usize = 7;

/// One day of the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayCell {
    /// The number shown (always 0–9 digits).
    pub number: Label,
    /// What a screen reader says ("22 November (today)").
    pub name: Label,
    /// A day of the previous or next month.
    pub muted: bool,
    /// Today.
    pub today: bool,
}

/// A month grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayGrid {
    /// Accessible name (the month).
    pub name: Label,
    /// Short weekday names, first day of the week first.
    pub headers: Vec<Label>,
    /// The days (at most [`MAX_DAY_CELLS`], rows of seven).
    pub cells: Vec<DayCell>,
    /// The selected day.
    pub selected: usize,
}

const ROW: f32 = size::TARGET_POINTER_MIN;
const GAP: f32 = Space::S4.points();
const HEADER: TextSpec = TextSpec::new(TypeSize::S12);
const DAY: TextSpec = TextSpec::new(TypeSize::S14);
const TODAY: TextSpec = TextSpec::new(TypeSize::S14).weight(Weight::Bold);

impl DayGrid {
    /// A grid; cells beyond [`MAX_DAY_CELLS`] and headers beyond seven are dropped.
    pub fn new(name: Label, mut headers: Vec<Label>, mut cells: Vec<DayCell>) -> DayGrid {
        headers.truncate(WEEK);
        cells.truncate(MAX_DAY_CELLS);
        let selected = cells.iter().position(|c| c.today).unwrap_or(0);
        DayGrid {
            name,
            headers,
            cells,
            selected,
        }
    }

    fn rows(&self) -> usize {
        self.cells.len().div_ceil(WEEK)
    }

    pub(crate) fn measure(&self, _cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let rows = self.rows() as f32 + 1.0;
        Size::new(avail, rows * ROW + (rows - 1.0).max(0.0) * GAP)
    }

    /// The rectangle of cell `index` (`None` beyond the cells); row 0 is the header.
    fn cell_rect(&self, rect: Rect, index: usize, direction: Direction) -> Rect {
        let width = ((rect.w - GAP * (WEEK as f32 - 1.0)) / WEEK as f32).max(0.0);
        let (row, column) = (index / WEEK, index % WEEK);
        let from_start = column as f32 * (width + GAP);
        let x = if direction.is_rtl() {
            rect.right() - from_start - width
        } else {
            rect.x + from_start
        };
        Rect::new(x, rect.y + row as f32 * (ROW + GAP), width, ROW)
    }

    /// The day under `pos`.
    pub(crate) fn part_at(&self, rect: Rect, pos: Point, direction: Direction) -> Option<usize> {
        (0..self.cells.len()).find(|&i| self.cell_rect(rect, i + WEEK, direction).contains(pos))
    }

    /// The day an arrow, Home or End leads to from the selected one (`towards_end` = the
    /// arrow points to the end of the reading direction).
    pub(crate) fn step(&self, key: crate::event::Key, towards_end: bool) -> Option<usize> {
        use crate::event::Key;
        let n = self.cells.len();
        let at = self.selected.min(n.saturating_sub(1));
        let target = match key {
            Key::ArrowLeft | Key::ArrowRight if towards_end => at + 1,
            Key::ArrowLeft | Key::ArrowRight => at.checked_sub(1)?,
            Key::ArrowDown => at + WEEK,
            Key::ArrowUp => at.checked_sub(WEEK)?,
            Key::Home => at - at % WEEK,
            Key::End => (at - at % WEEK + WEEK - 1).min(n.saturating_sub(1)),
            _ => return None,
        };
        (target < n && target != at).then_some(target)
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        let direction = p.env().direction;
        let muted = p.color(ColorRole::TextMuted);
        for (i, header) in self.headers.iter().enumerate() {
            let r = self.cell_rect(rect, i, direction);
            style::draw_label(p, cx, header, HEADER, r, TextAlign::Center, muted);
        }
        let previous = p.set_opacity(v.opacity());
        for (i, cell) in self.cells.iter().enumerate() {
            let r = self.cell_rect(rect, i + WEEK, direction);
            let chosen = i == self.selected;
            let (fill, edge, fg, spec) = if cell.today {
                (
                    Some(ColorRole::Accent),
                    Some(ColorRole::AccentText),
                    ColorRole::OnAccent,
                    TODAY,
                )
            } else if chosen {
                (
                    Some(ColorRole::Hover),
                    Some(ColorRole::ControlBorder),
                    ColorRole::Text,
                    DAY,
                )
            } else if cell.muted {
                (None, None, ColorRole::TextMuted, DAY)
            } else {
                (None, None, ColorRole::Text, DAY)
            };
            if let Some(fill) = fill {
                let fill = p.color(fill);
                p.fill(r, Some(Radius::Control), fill);
            }
            if let Some(edge) = edge {
                let edge = p.color(edge);
                p.stroke(r, Some(Radius::Control), p.hairline(), edge);
            }
            let fg = p.color(fg);
            style::draw_label(p, cx, &cell.number, spec, r, TextAlign::Center, fg);
            if chosen && v.focused {
                style::focus_ring(p, r, Radius::Control);
            }
        }
        p.set_opacity(previous);
    }

    /// What a screen reader says as the value: the selected day.
    pub fn selected_name(&self) -> Option<Label> {
        self.cells.get(self.selected).map(|c| c.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Key;

    fn grid(n: usize) -> DayGrid {
        let cells = (0..n)
            .map(|i| DayCell {
                number: Label::data(&i.to_string()),
                name: Label::data(&i.to_string()),
                muted: false,
                today: i == 9,
            })
            .collect();
        DayGrid::new(Label::data("m"), Vec::new(), cells)
    }

    #[test]
    fn arrows_move_in_rows_and_columns_and_stop_at_the_edges() {
        let g = grid(35);
        assert_eq!(g.selected, 9);
        assert_eq!(g.step(Key::ArrowRight, true), Some(10));
        assert_eq!(g.step(Key::ArrowLeft, false), Some(8));
        assert_eq!(g.step(Key::ArrowDown, true), Some(16));
        assert_eq!(g.step(Key::ArrowUp, true), Some(2));
        assert_eq!(g.step(Key::Home, true), Some(7));
        assert_eq!(g.step(Key::End, true), Some(13));
        let mut last = grid(35);
        last.selected = 34;
        assert_eq!(last.step(Key::ArrowRight, true), None);
        assert_eq!(last.step(Key::ArrowDown, true), None);
        let mut first = grid(35);
        first.selected = 0;
        assert_eq!(first.step(Key::ArrowLeft, false), None);
        assert_eq!(first.step(Key::ArrowUp, true), None);
        assert_eq!(grid(0).step(Key::ArrowRight, true), None);
        assert_eq!(grid(60).cells.len(), MAX_DAY_CELLS);
    }

    #[test]
    fn columns_follow_the_reading_direction() {
        let g = grid(35);
        let rect = Rect::new(0.0, 0.0, 7.0 * 32.0 + 6.0 * 4.0, 200.0);
        let ltr = g.cell_rect(rect, WEEK, Direction::Ltr);
        let rtl = g.cell_rect(rect, WEEK, Direction::Rtl);
        assert_eq!(ltr.x, 0.0);
        assert_eq!(rtl.right(), rect.right());
        let p = Point::new(rtl.x + 1.0, rtl.y + 1.0);
        assert_eq!(g.part_at(rect, p, Direction::Rtl), Some(0));
        assert_eq!(g.part_at(rect, Point::new(1.0, 1.0), Direction::Ltr), None);
    }
}
