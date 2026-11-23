//! Table: rows of cells under column headers. Rows are data (not nodes), so a table of
//! thousands of rows costs one node; only the rows inside the drawn area are painted.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, state, ColorRole, InputMode, Radius, Space};
use ultar_i18n::Direction;

/// Most rows a table holds.
pub const MAX_TABLE_ROWS: usize = 100_000;
/// Most columns a table holds.
pub const MAX_TABLE_COLUMNS: usize = 16;

/// Where the keyboard is inside a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableCursor {
    /// On a column header (sorting).
    Header(usize),
    /// On the rows (the selected row).
    #[default]
    Rows,
}

/// A table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// Accessible name (catalog key).
    pub name: Label,
    /// Column headers, start to end.
    pub columns: Vec<Label>,
    /// Rows of cells (missing cells are empty).
    pub rows: Vec<Vec<Label>>,
    /// The selected row.
    pub selected: Option<usize>,
    /// The column the rows are sorted by and whether ascending (the application sorts).
    pub sort: Option<(usize, bool)>,
    /// Where the keyboard is.
    pub cursor: TableCursor,
    /// `true` when several rows can be marked (Ctrl toggles, Shift extends).
    pub multi: bool,
    /// The marked rows of a multi-selection table, ascending (read them with
    /// `Ui::selected_rows`).
    pub(crate) chosen: Vec<usize>,
    /// Where a Shift range starts.
    pub(crate) anchor: Option<usize>,
}

const PAD: Space = Space::S12;
const MARK: f32 = size::ICON_SMALL;

/// What a pointer is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TablePart {
    Header(usize),
    Row(usize),
}

impl TablePart {
    /// The part as a number for the press slots: headers first, then rows.
    pub(crate) fn encode(self) -> usize {
        match self {
            TablePart::Header(c) => c,
            TablePart::Row(r) => MAX_TABLE_COLUMNS + r,
        }
    }

    /// The inverse of [`TablePart::encode`].
    pub(crate) fn decode(part: usize) -> TablePart {
        if part < MAX_TABLE_COLUMNS {
            TablePart::Header(part)
        } else {
            TablePart::Row(part - MAX_TABLE_COLUMNS)
        }
    }
}

impl Table {
    /// A table with column headers and no rows (columns beyond the limit are dropped).
    pub fn new(name: Label, mut columns: Vec<Label>) -> Table {
        columns.truncate(MAX_TABLE_COLUMNS);
        Table {
            name,
            columns,
            rows: Vec::new(),
            selected: None,
            sort: None,
            cursor: TableCursor::Rows,
            multi: false,
            chosen: Vec::new(),
            anchor: None,
        }
    }

    /// The same table with multi-selection: Ctrl with a click or Space marks a row, Shift
    /// with a click or an arrow marks a range; every marked row is announced as selected.
    pub fn multi(mut self) -> Table {
        self.multi = true;
        self
    }

    /// `true` when row `row` is selected (marked in a multi-selection table).
    pub(crate) fn is_selected(&self, row: usize) -> bool {
        if self.multi {
            self.chosen.binary_search(&row).is_ok()
        } else {
            self.selected == Some(row)
        }
    }

    /// The same table with rows (rows beyond the limit are dropped).
    pub fn rows(mut self, mut rows: Vec<Vec<Label>>) -> Table {
        rows.truncate(MAX_TABLE_ROWS);
        self.rows = rows;
        self
    }

    /// Height of one row (and of the header) for an input mode.
    pub(crate) fn row_height(mode: InputMode) -> f32 {
        mode.min_target()
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let rows = self.rows.len() as f32 + 1.0;
        Size::new(avail, rows * Table::row_height(cx.mode))
    }

    /// The rectangle of column `column` within a band `band` (a row or the header).
    pub(crate) fn cell(&self, band: Rect, column: usize, direction: Direction) -> Rect {
        let width = band.w / self.columns.len().max(1) as f32;
        let offset = column as f32 * width;
        let x = if direction.is_rtl() {
            band.right() - offset - width
        } else {
            band.x + offset
        };
        Rect::new(x, band.y, width, band.h)
    }

    /// The band of row `row` (`None` = the header).
    pub(crate) fn band(&self, rect: Rect, row: Option<usize>, mode: InputMode) -> Rect {
        let h = Table::row_height(mode);
        let index = row.map_or(0.0, |r| r as f32 + 1.0);
        Rect::new(rect.x, rect.y + index * h, rect.w, h)
    }

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
        let h = Table::row_height(mode);
        let index = ((pos.y - rect.y) / h).floor() as usize;
        if index == 0 {
            let header = self.band(rect, None, mode);
            let column =
                (0..self.columns.len()).find(|c| self.cell(header, *c, direction).contains(pos))?;
            return Some(TablePart::Header(column).encode());
        }
        (index - 1 < self.rows.len()).then(|| TablePart::Row(index - 1).encode())
    }

    /// The sort order after choosing column `column`: ascending first, then the other way.
    pub(crate) fn next_sort(&self, column: usize) -> (usize, bool) {
        match self.sort {
            Some((c, ascending)) if c == column => (column, !ascending),
            _ => (column, true),
        }
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
        let header = self.band(rect, None, cx.mode);
        self.paint_header(p, cx, header, v, hover);
        let h = Table::row_height(cx.mode);
        let visible = p.visible().intersect(&rect);
        let first = ((visible.y - rect.y) / h - 1.0).floor().max(0.0) as usize;
        let last = (((visible.bottom() - rect.y) / h).ceil() as usize).min(self.rows.len());
        for row in first..last {
            let band = self.band(rect, Some(row), cx.mode);
            let hovered = hover == Some(TablePart::Row(row).encode());
            self.paint_row(p, cx, band, row, hovered);
            let focused =
                v.focused && self.cursor == TableCursor::Rows && self.selected == Some(row);
            if focused {
                let ring = p.color(state::FOCUS_RING);
                p.stroke(band, Some(Radius::Control), size::FOCUS_RING, ring);
            }
        }
        p.set_opacity(previous);
    }

    fn paint_header(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        header: Rect,
        v: &Visual,
        hover: Option<usize>,
    ) {
        let direction = p.env().direction;
        let (line, border) = (p.color(ColorRole::ControlBorder), p.hairline());
        let rule = Rect::new(header.x, header.bottom() - border, header.w, border);
        p.fill(rule, None, line);
        for (column, label) in self.columns.iter().enumerate() {
            let cell = self.cell(header, column, direction);
            if hover == Some(TablePart::Header(column).encode()) {
                let bg = p.color(ColorRole::Hover);
                p.fill(cell, Some(Radius::Control), bg);
            }
            let mut inner = style::inset_inline(cell, PAD);
            if let Some((_, ascending)) = self.sort.filter(|(c, _)| *c == column) {
                let slot = style::take_end(&mut inner, MARK, direction);
                let turns = if ascending {
                    QuarterTurns::Two
                } else {
                    QuarterTurns::None
                };
                let accent = p.color(ColorRole::AccentText);
                p.icon(IconId::ChevronDown, slot, MARK, turns, accent);
            }
            let muted = p.color(ColorRole::TextMuted);
            let spec = style::CONTROL_TEXT.weight(Weight::Semibold);
            style::draw_label(p, cx, label, spec, inner, TextAlign::Start, muted);
            if v.focused && self.cursor == TableCursor::Header(column) {
                let ring = p.color(state::FOCUS_RING);
                p.stroke(cell, Some(Radius::Control), size::FOCUS_RING, ring);
            }
        }
    }

    fn paint_row(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        band: Rect,
        row: usize,
        hovered: bool,
    ) {
        let direction = p.env().direction;
        let selected = self.selected == Some(row);
        if selected || hovered {
            let role = if selected {
                ColorRole::Line
            } else {
                ColorRole::Hover
            };
            let bg = p.color(role);
            p.fill(band, Some(Radius::Control), bg);
        }
        let cells = self.rows.get(row).map_or(&[][..], Vec::as_slice);
        for (column, label) in cells.iter().enumerate().take(self.columns.len()) {
            let inner = style::inset_inline(self.cell(band, column, direction), PAD);
            let weight = if selected {
                Weight::Semibold
            } else {
                Weight::Regular
            };
            let color = p.color(ColorRole::Text);
            let spec = style::CONTROL_TEXT.weight(weight);
            style::draw_label(p, cx, label, spec, inner, TextAlign::Start, color);
        }
        let (line, border) = (p.color(ColorRole::Line), p.hairline());
        p.fill(
            Rect::new(band.x, band.bottom() - border, band.w, border),
            None,
            line,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        let columns = vec![Label::data("a"), Label::data("b")];
        let rows = (0..5).map(|i| vec![Label::data(&i.to_string())]).collect();
        Table::new(Label::key("t"), columns).rows(rows)
    }

    #[test]
    fn parts_columns_and_sorting() {
        let t = table();
        let mode = InputMode::Pointer;
        let h = Table::row_height(mode);
        let rect = Rect::new(0.0, 0.0, 200.0, 6.0 * h);
        let at = |x, y, d| {
            t.part_at(rect, Point::new(x, y), mode, d)
                .map(TablePart::decode)
        };
        assert_eq!(at(10.0, 1.0, Direction::Ltr), Some(TablePart::Header(0)));
        assert_eq!(at(10.0, 1.0, Direction::Rtl), Some(TablePart::Header(1)));
        assert_eq!(at(10.0, h + 1.0, Direction::Ltr), Some(TablePart::Row(0)));
        assert_eq!(at(10.0, 5.5 * h, Direction::Ltr), Some(TablePart::Row(4)));
        assert_eq!(at(10.0, 9.0 * h, Direction::Ltr), None);
        assert_eq!(t.next_sort(1), (1, true));
        let sorted = Table {
            sort: Some((1, true)),
            ..table()
        };
        assert_eq!(sorted.next_sort(1), (1, false));
        assert_eq!(sorted.next_sort(0), (0, true));
        let many = Table::new(Label::key("t"), vec![Label::data("x"); 40]);
        assert_eq!(many.columns.len(), MAX_TABLE_COLUMNS);
        assert_eq!(
            TablePart::decode(TablePart::Row(7).encode()),
            TablePart::Row(7)
        );
    }
}
