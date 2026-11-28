//! What the user does to cells: copy, cut, paste, fill, clear, insert and delete rows and
//! columns, sort, formats, sheets, find and replace, row heights and column widths.

use crate::core::Core;
use crate::frame::{col_chars, MIN_COL, MIN_ROW};
use crate::sel::Span;
use ultar_office::sheet::Sheet;
use ultar_office::sheet_edit::{parse_tsv, NameError};
use ultar_office::sheet_find::{replace_text, FindOpts};
use ultar_office::sheet_refs::{relocate, Axis, MAX_COLS, MAX_ROWS};
use ultar_office::style::{Align, CellStyle};
use ultar_ui::widgets::Label;

/// Largest block pasted (more is cut off with a message).
const MAX_PASTE_CELLS: usize = 1_000_000;

/// What the last copy took: the shown text (to recognise our own paste) and the cells with
/// their formulas and formats.
#[derive(Debug, Clone)]
pub struct Clip {
    /// The text that went to the clipboard.
    pub text: String,
    /// Where the block was.
    pub origin: (u32, u32),
    /// Rows of `(content, format)`.
    pub cells: Vec<Vec<(String, CellStyle)>>,
}

/// The find and replace bar.
#[derive(Debug, Clone, Default)]
pub struct Find {
    /// The bar is open.
    pub open: bool,
    /// With the replace field.
    pub replace: bool,
    /// Text to find.
    pub needle: String,
    /// Replacement.
    pub with: String,
    /// Match case.
    pub match_case: bool,
    /// Only whole cells.
    pub whole_cell: bool,
    /// Number of hits at the last search (shown in the bar).
    pub hits: usize,
}

impl Find {
    fn opts(&self) -> FindOpts {
        FindOpts {
            match_case: self.match_case,
            whole_cell: self.whole_cell,
            in_values: true,
            // Arabic: letter forms and diacritics do not matter when searching
            normalize: Some(ultar_i18n::normalize_for_search),
        }
    }
}

/// A number format offered in the tool bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumFmt {
    /// As typed.
    General,
    /// `#,##0.00`.
    Number,
    /// `0.00%`.
    Percent,
    /// `$#,##0.00`.
    Currency,
    /// `yyyy-mm-dd`.
    Date,
}

impl NumFmt {
    /// The Excel format code.
    pub fn code(self) -> &'static str {
        match self {
            NumFmt::General => "",
            NumFmt::Number => "#,##0.00",
            NumFmt::Percent => "0.00%",
            NumFmt::Currency => "$#,##0.00",
            NumFmt::Date => "yyyy-mm-dd",
        }
    }
}

impl Core {
    /// The selected rectangle limited to the used area (whole rows and columns do not mean
    /// a million cells).
    pub fn used_rect(&self) -> (u32, u32, u32, u32) {
        self.sel.used(self.book.sheet().extent())
    }

    /// Sum, average and count of the numbers in a selection of several cells (the status
    /// line shows them); `None` for one cell or when it holds no numbers.
    pub fn stats(&mut self) -> Option<(f64, f64, usize)> {
        if self.sel.single() {
            return None;
        }
        let (r0, c0, r1, c1) = self.used_rect();
        let keys: Vec<(u32, u32)> = self
            .book
            .sheet()
            .cells
            .range((r0, 0)..=(r1, u32::MAX))
            .map(|(&k, _)| k)
            .filter(|&(_, c)| (c0..=c1).contains(&c))
            .take(200_000)
            .collect();
        let (mut sum, mut n) = (0.0, 0usize);
        for (r, c) in keys {
            if let ultar_office::sheet::Value::Num(x) = self.book.value(r, c) {
                sum += x;
                n += 1;
            }
        }
        (n > 0).then(|| (sum, sum / n as f64, n))
    }

    // ---------------------------------------------------------------- clipboard

    /// Copies the selection; returns the text for the clipboard.
    pub fn copy(&mut self) -> String {
        let rect = self.used_rect();
        let (r0, c0, r1, c1) = rect;
        let sh = self.book.sheet();
        let cells = (r0..=r1)
            .map(|r| (c0..=c1).map(|c| (sh.get(r, c).to_owned(), sh.style(r, c))).collect())
            .collect();
        let text = self.book.wb.to_tsv(self.book.active, rect, false);
        self.clip = Some(Clip { text: text.clone(), origin: (r0, c0), cells });
        self.say("sheet.copied");
        text
    }

    /// Copies the selection and clears it.
    pub fn cut(&mut self) -> String {
        let text = self.copy();
        self.clear_selection();
        self.say("sheet.cut");
        text
    }

    /// Pastes text from the clipboard at the active cell: our own copy with formulas and
    /// formats (references move with the block), text from other apps as tab-separated cells.
    pub fn paste(&mut self, text: &str) {
        if self.edit.is_some() {
            self.edit_paste(text);
            return;
        }
        let (r0, c0, ..) = self.sel.rect();
        let own = self.clip.clone().filter(|c| c.text == text);
        let block: Vec<Vec<(String, Option<CellStyle>)>> = match own {
            Some(c) => {
                let (dr, dc) = (i64::from(r0) - i64::from(c.origin.0), i64::from(c0) - i64::from(c.origin.1));
                c.cells
                    .into_iter()
                    .map(|row| row.into_iter().map(|(raw, st)| (relocate(&raw, dr, dc), Some(st))).collect())
                    .collect()
            }
            None => parse_tsv(text)
                .into_iter()
                .map(|row| row.into_iter().map(|f| (f, None)).collect())
                .collect(),
        };
        let mut cells = Vec::new();
        let mut cut_off = false;
        let (mut maxr, mut maxc) = (r0, c0);
        for (i, row) in block.into_iter().enumerate() {
            for (j, (raw, st)) in row.into_iter().enumerate() {
                let (r, c) = (r0 + i as u32, c0 + j as u32);
                if r >= MAX_ROWS || c >= MAX_COLS || cells.len() >= MAX_PASTE_CELLS {
                    cut_off = true;
                    continue;
                }
                maxr = maxr.max(r);
                maxc = maxc.max(c);
                cells.push(((r, c), raw, st));
            }
        }
        if cells.is_empty() {
            return;
        }
        self.book.put(cells);
        self.sel.anchor = (r0, c0);
        self.sel.active = (r0, c0);
        self.sel.cur = (maxr, maxc);
        self.sel.span = Span::Cells;
        self.message = Some(if cut_off { Label::key("sheet.paste_cut") } else { Label::key("sheet.pasted") });
        self.reveal();
    }

    /// Clears the content of the selection (formats stay).
    pub fn clear_selection(&mut self) {
        let (r0, c0, r1, c1) = self.used_rect();
        let sh = self.book.sheet();
        let cells: Vec<_> = sh
            .cells
            .range((r0, 0)..=(r1, u32::MAX))
            .filter(|(&(_, c), _)| (c0..=c1).contains(&c))
            .map(|(&k, _)| (k, String::new(), None))
            .collect();
        self.book.put(cells);
    }

    /// Fills the first row (or column) of the selection into the rest.
    pub fn fill(&mut self, down: bool) {
        let (r0, c0, r1, c1) = self.used_rect();
        let sh = self.book.sheet();
        let mut cells = Vec::new();
        if down {
            for c in c0..=c1 {
                let (raw, st) = (sh.get(r0, c).to_owned(), sh.style(r0, c));
                for r in r0 + 1..=r1 {
                    cells.push(((r, c), relocate(&raw, i64::from(r - r0), 0), Some(st.clone())));
                }
            }
        } else {
            for r in r0..=r1 {
                let (raw, st) = (sh.get(r, c0).to_owned(), sh.style(r, c0));
                for c in c0 + 1..=c1 {
                    cells.push(((r, c), relocate(&raw, 0, i64::from(c - c0)), Some(st.clone())));
                }
            }
        }
        self.book.put(cells);
    }

    // ---------------------------------------------------------------- rows and columns

    /// Inserts as many rows or columns as are selected, before the selection.
    pub fn insert_lines(&mut self, axis: Axis) {
        let (r0, c0, r1, c1) = self.sel.rect();
        let (at, n) = match axis {
            Axis::Rows => (r0, r1 - r0 + 1),
            Axis::Cols => (c0, c1 - c0 + 1),
        };
        let sheet = self.book.active;
        let ok = self.book.structural(|wb, _| wb.insert_lines(sheet, axis, at, n));
        if !ok {
            self.say("sheet.no_room");
        }
    }

    /// Deletes the selected rows or columns.
    pub fn delete_lines(&mut self, axis: Axis) {
        let (r0, c0, r1, c1) = self.sel.rect();
        let (at, n) = match axis {
            Axis::Rows => (r0, r1 - r0 + 1),
            Axis::Cols => (c0, c1 - c0 + 1),
        };
        let sheet = self.book.active;
        self.book.structural(|wb, _| wb.delete_lines(sheet, axis, at, n));
        let (r, c) = self.sel.active;
        self.sel.select(r, c);
    }

    /// Sorts the selected rows by the column of the active cell. One cell selected: the whole
    /// used table below the frozen header; one column selected: its neighbours sort along.
    pub fn sort(&mut self, ascending: bool) {
        let (mut r0, mut c0, mut r1, mut c1) = self.used_rect();
        let key = self.sel.active.1;
        let (rows, cols) = self.book.sheet().extent();
        let frozen = self.book.sheet().frozen_rows;
        if self.sel.single() {
            (r0, r1) = (0, rows.saturating_sub(1).max(r0));
        }
        if c0 == c1 {
            (c0, c1) = (0, cols.saturating_sub(1).max(c1));
        }
        r0 = r0.max(frozen);
        let sheet = self.book.active;
        let result = self.book.structural_try(|wb, _| {
            wb.sort_rows(sheet, (r0, c0, r1, c1), key, ascending, &ultar_i18n::compare)
        });
        if result.is_err() {
            self.say("sheet.sort_failed");
        }
    }
    /// Sets the width of a column from a width in points (a drag).
    pub fn set_col_width(&mut self, c: u32, points: f32) {
        let w = col_chars(points.max(MIN_COL));
        let sheet = self.book.active;
        self.book.edit_view(sheet, |sh| {
            sh.widths.insert(c, w);
        });
    }

    /// Sets the height of a row in points.
    pub fn set_row_height(&mut self, r: u32, points: f32) {
        let sheet = self.book.active;
        self.book.edit_view(sheet, |sh| {
            sh.heights.insert(r, points.max(MIN_ROW));
        });
    }

    // ---------------------------------------------------------------- formats

    fn restyle(&mut self, f: &dyn Fn(&mut CellStyle)) {
        let rect = self.used_rect();
        self.book.restyle(rect, f);
    }

    fn cur_style(&self) -> CellStyle {
        self.book.sheet().style(self.sel.active.0, self.sel.active.1)
    }

    /// Bold on or off for the selection.
    pub fn toggle_bold(&mut self) {
        let on = !self.cur_style().bold;
        self.restyle(&|s| s.bold = on);
    }

    /// Italic on or off (kept and saved; the text engine has no italic face yet).
    pub fn toggle_italic(&mut self) {
        let on = !self.cur_style().italic;
        self.restyle(&|s| s.italic = on);
    }

    /// Wrapping on or off.
    pub fn toggle_wrap(&mut self) {
        let on = !self.cur_style().wrap;
        self.restyle(&|s| s.wrap = on);
    }

    /// Text colour (`None`: the theme's).
    pub fn set_color(&mut self, color: Option<u32>) {
        self.restyle(&|s| s.color = color);
    }

    /// Fill colour.
    pub fn set_fill(&mut self, fill: Option<u32>) {
        self.restyle(&|s| s.fill = fill);
    }

    /// Alignment.
    pub fn set_align(&mut self, align: Align) {
        self.restyle(&|s| s.align = align);
    }

    /// Number format.
    pub fn set_num_fmt(&mut self, f: NumFmt) {
        self.restyle(&|s| s.num_fmt = f.code().to_owned());
    }

    /// Freezes the first row, or lets it go.
    pub fn toggle_freeze_row(&mut self) {
        let sheet = self.book.active;
        self.book.edit_view(sheet, |sh| sh.frozen_rows = u32::from(sh.frozen_rows == 0));
        self.first.0 = self.first.0.max(0);
    }

    /// Freezes the first column, or lets it go.
    pub fn toggle_freeze_col(&mut self) {
        let sheet = self.book.active;
        self.book.edit_view(sheet, |sh| sh.frozen_cols = u32::from(sh.frozen_cols == 0));
    }

    /// Turns the sheet around: column A at the right edge, or back.
    pub fn toggle_rtl(&mut self) {
        let sheet = self.book.active;
        self.book.edit_view(sheet, |sh| sh.rtl = !sh.rtl);
    }

    // ---------------------------------------------------------------- sheets

    /// Adds a sheet and shows it.
    pub fn add_sheet(&mut self) -> usize {
        let name = self.book.wb.new_sheet_name("Sheet");
        let rtl = self.book.sheet().rtl;
        let i = self.book.structural(|wb, _| {
            let mut sh = Sheet::new(&name);
            sh.rtl = rtl;
            wb.sheets.push(sh);
            wb.sheets.len() - 1
        });
        self.views.resize(self.book.wb.sheets.len(), (crate::sel::Sel::default(), (0, 0)));
        self.show_sheet(i);
        i
    }

    /// Renames sheet `i` (formulas that name it follow).
    pub fn rename_sheet(&mut self, i: usize, name: &str) -> Result<(), NameError> {
        self.book.structural_try(|wb, _| wb.rename_sheet(i, name))
    }

    /// Deletes sheet `i` (undo brings it back).
    pub fn delete_sheet(&mut self, i: usize) -> Result<(), NameError> {
        if self.book.wb.sheets.len() <= 1 {
            return Err(NameError::Last);
        }
        self.cancel();
        self.views.resize(self.book.wb.sheets.len(), (crate::sel::Sel::default(), (0, 0)));
        let result = self.book.structural_try(|wb, active| {
            wb.delete_sheet(i)?;
            if *active >= i {
                *active = active.saturating_sub(1);
            }
            Ok(())
        });
        self.views.truncate(self.book.wb.sheets.len());
        (self.sel, self.first) = self.views.get(self.book.active).copied().unwrap_or_default();
        result
    }

    // ---------------------------------------------------------------- find and replace

    /// Counts the hits and selects the next one after the active cell (`forward`) or before.
    pub fn find_next(&mut self, forward: bool) -> bool {
        let opts = self.find.opts();
        let mut hits = self.book.wb.find_all(None, &self.find.needle, &opts);
        self.find.hits = hits.len();
        if hits.is_empty() {
            self.message = Some(Label::key("sheet.not_found"));
            return false;
        }
        hits.sort();
        let here = (self.book.active, self.sel.active.0, self.sel.active.1);
        let pick = if forward {
            hits.iter().find(|h| **h > here).or(hits.first())
        } else {
            hits.iter().rev().find(|h| **h < here).or(hits.last())
        };
        let Some(&(s, r, c)) = pick else {
            return false;
        };
        self.show_sheet(s);
        self.sel.select(r, c);
        self.reveal();
        true
    }

    /// Replaces in the active cell when it matches, then goes on to the next hit.
    pub fn replace_one(&mut self) {
        let opts = self.find.opts();
        let (r, c) = self.sel.active;
        let raw = self.raw();
        if let Some(new) = replace_text(&raw, &self.find.needle, &self.find.with, &opts) {
            self.book.set(r, c, &new);
        }
        self.find_next(true);
    }

    /// Replaces every hit of the workbook (one undo step); returns the number of cells.
    pub fn replace_all(&mut self) -> usize {
        let opts = self.find.opts();
        let (needle, with) = (self.find.needle.clone(), self.find.with.clone());
        let hits = self.book.wb.find_all(None, &needle, &opts);
        let mut done = 0;
        self.book.structural(|wb, _| {
            for (s, r, c) in &hits {
                let raw = wb.sheets[*s].get(*r, *c).to_owned();
                if let Some(new) = replace_text(&raw, &needle, &with, &opts) {
                    wb.sheets[*s].set(*r, *c, &new);
                    done += 1;
                }
            }
        });
        self.message = Some(Label::key_with("sheet.replaced", &[("n", &done.to_string())]));
        done
    }
}
