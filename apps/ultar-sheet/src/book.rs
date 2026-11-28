//! The open workbook with its undo history, its recalculation and its file state.
//!
//! Every change goes through here: a change of cells is one history step (the old and the
//! new content), a structural change (rows, columns, sort, sheets) keeps a copy of the whole
//! workbook. Recalculation is by dependency: only cells that depend on a change are computed
//! again ([`ultar_office::calc::Calc`]).

use crate::file::Format;
use std::path::PathBuf;
use ultar_office::calc::Calc;
use ultar_office::numfmt::{self, Names};
use ultar_office::sheet::{excel_now, Value, Workbook};
use ultar_office::style::CellStyle;

/// Steps kept for undo.
const HISTORY: usize = 100;

/// One cell of a change: content and formatting before and after.
#[derive(Debug, Clone)]
struct Edit {
    at: (u32, u32),
    old: (String, CellStyle),
    new: (String, CellStyle),
}

#[derive(Debug, Clone)]
enum Step {
    /// Cells of one sheet.
    Cells { sheet: usize, edits: Vec<Edit> },
    /// The whole workbook before (and after, when undone) a structural change.
    Whole { book: Box<Workbook>, active: usize },
}

/// How a cell is shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    /// The text in the cell.
    pub text: String,
    /// A number (right-aligned in a general cell).
    pub number: bool,
    /// An error value (`#DIV/0!` ...).
    pub error: bool,
}

/// The workbook of the window.
pub struct Book {
    /// The cells.
    pub wb: Workbook,
    /// The shown sheet.
    pub active: usize,
    /// The file (none for a new workbook).
    pub path: Option<PathBuf>,
    /// How the file is stored.
    pub format: Format,
    calc: Calc,
    undo: Vec<Step>,
    redo: Vec<Step>,
    dirty: bool,
    /// Counts every change (autosave looks at it).
    pub version: u64,
    /// Month and weekday names for number formats.
    pub names: Names,
}

impl Book {
    /// A new, empty workbook.
    pub fn new() -> Book {
        Book::of(Workbook::new(), None, Format::Xlsx)
    }

    /// A workbook read from `path`.
    pub fn of(wb: Workbook, path: Option<PathBuf>, format: Format) -> Book {
        Book {
            wb,
            active: 0,
            path,
            format,
            calc: Calc::new(excel_now()),
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            version: 0,
            names: Names::default(),
        }
    }

    /// Unsaved changes?
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// The file is saved (or the changes are given up).
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// File name of the workbook.
    pub fn name(&self) -> Option<String> {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// The shown sheet.
    pub fn sheet(&self) -> &ultar_office::sheet::Sheet {
        &self.wb.sheets[self.active.min(self.wb.sheets.len() - 1)]
    }

    /// The value of a cell of the shown sheet.
    pub fn value(&mut self, r: u32, c: u32) -> Value {
        self.calc.value(&self.wb, self.active, r, c)
    }

    /// The value of a cell of any sheet.
    pub fn value_in(&mut self, sheet: usize, r: u32, c: u32) -> Value {
        self.calc.value(&self.wb, sheet, r, c)
    }

    /// The cell as it is shown, with its number format.
    pub fn shown(&mut self, r: u32, c: u32) -> Shown {
        let v = self.value(r, c);
        let code = self.sheet().style(r, c).num_fmt;
        match v {
            Value::Empty => Shown {
                text: String::new(),
                number: false,
                error: false,
            },
            Value::Num(n) => Shown {
                text: numfmt::format(n, &code, &self.names),
                number: true,
                error: false,
            },
            Value::Err(e) => Shown {
                text: e,
                number: false,
                error: true,
            },
            other => Shown {
                text: other.text(),
                number: false,
                error: false,
            },
        }
    }

    fn push(&mut self, step: Step) {
        self.undo.push(step);
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.touch();
    }

    fn touch(&mut self) {
        self.dirty = true;
        self.version += 1;
    }

    /// Can undo / redo?
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Can redo?
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Sets cells of the shown sheet (content and optionally formatting) as one step.
    pub fn put(&mut self, cells: Vec<((u32, u32), String, Option<CellStyle>)>) {
        let sheet = self.active;
        let mut edits = Vec::new();
        for (at, raw, style) in cells {
            let sh = &self.wb.sheets[sheet];
            let old = (sh.get(at.0, at.1).to_owned(), sh.style(at.0, at.1));
            let new = (raw, style.unwrap_or_else(|| old.1.clone()));
            if old != new {
                edits.push(Edit { at, old, new });
            }
        }
        if edits.is_empty() {
            return;
        }
        self.apply(sheet, &edits, true);
        self.push(Step::Cells { sheet, edits });
    }

    fn apply(&mut self, sheet: usize, edits: &[Edit], forward: bool) {
        let mut keys = Vec::with_capacity(edits.len());
        for e in edits {
            let (raw, style) = if forward { &e.new } else { &e.old };
            let sh = &mut self.wb.sheets[sheet];
            sh.set(e.at.0, e.at.1, raw);
            sh.set_style(e.at.0, e.at.1, style.clone());
            keys.push((sheet, e.at.0, e.at.1));
        }
        self.calc.changed(&keys);
    }

    /// Sets one cell's content.
    pub fn set(&mut self, r: u32, c: u32, raw: &str) {
        self.put(vec![((r, c), raw.to_owned(), None)]);
    }

    /// Changes the formatting of every cell of the rectangle.
    pub fn restyle(&mut self, rect: (u32, u32, u32, u32), f: &dyn Fn(&mut CellStyle)) {
        let (r0, c0, r1, c1) = rect;
        let mut cells = Vec::new();
        let sh = self.sheet();
        let mut visit = |r: u32, c: u32| {
            let mut st = sh.style(r, c);
            f(&mut st);
            cells.push(((r, c), sh.get(r, c).to_owned(), Some(st)));
        };
        // a whole row or column: only the cells that exist, the rest is not touched
        let area = u64::from(r1 - r0 + 1) * u64::from(c1 - c0 + 1);
        if area <= 100_000 {
            for r in r0..=r1 {
                for c in c0..=c1 {
                    visit(r, c);
                }
            }
        } else {
            let keys: Vec<(u32, u32)> = sh
                .cells
                .keys()
                .chain(sh.styles.keys())
                .copied()
                .filter(|&(r, c)| (r0..=r1).contains(&r) && (c0..=c1).contains(&c))
                .collect();
            for (r, c) in keys {
                visit(r, c);
            }
        }
        self.put(cells);
    }

    /// Runs a structural change on the workbook; the whole workbook is kept for undo.
    pub fn structural<R>(&mut self, f: impl FnOnce(&mut Workbook, &mut usize) -> R) -> R {
        let before = Box::new(self.wb.clone());
        let active = self.active;
        let out = f(&mut self.wb, &mut self.active);
        self.active = self.active.min(self.wb.sheets.len().saturating_sub(1));
        self.calc.clear();
        self.push(Step::Whole {
            book: before,
            active,
        });
        out
    }

    /// Like [`Book::structural`], for a change that can be refused: a refusal leaves the
    /// workbook and the history as they were.
    pub fn structural_try<R, E>(
        &mut self,
        f: impl FnOnce(&mut Workbook, &mut usize) -> Result<R, E>,
    ) -> Result<R, E> {
        let before = Box::new(self.wb.clone());
        let active = self.active;
        match f(&mut self.wb, &mut self.active) {
            Ok(out) => {
                self.active = self.active.min(self.wb.sheets.len().saturating_sub(1));
                self.calc.clear();
                self.push(Step::Whole {
                    book: before,
                    active,
                });
                Ok(out)
            }
            Err(e) => {
                self.wb = *before;
                self.active = active;
                Err(e)
            }
        }
    }

    /// Changes how a sheet looks (widths, heights, frozen parts, direction). Not a step of
    /// the history, but the workbook is changed.
    pub fn edit_view(&mut self, sheet: usize, f: impl FnOnce(&mut ultar_office::sheet::Sheet)) {
        if let Some(sh) = self.wb.sheets.get_mut(sheet) {
            f(sh);
            self.touch();
        }
    }

    /// Takes another workbook's cells as unsaved changes of this one (a recovered copy).
    pub fn adopt(&mut self, wb: Workbook) {
        self.wb = wb;
        self.active = 0;
        self.calc.clear();
        self.undo.clear();
        self.redo.clear();
        self.touch();
    }

    /// Undoes the last step; the sheet it changed becomes the shown one.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else {
            return false;
        };
        let back = self.revert(step);
        self.redo.push(back);
        self.touch();
        true
    }

    /// Redoes the last undone step.
    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        let back = self.revert(step);
        self.undo.push(back);
        self.touch();
        true
    }

    /// Applies a step in the other direction and returns the step that reverts it.
    fn revert(&mut self, step: Step) -> Step {
        match step {
            Step::Cells { sheet, edits } => {
                // the undo of an undo: swap old and new
                let swapped: Vec<Edit> = edits
                    .iter()
                    .map(|e| Edit {
                        at: e.at,
                        old: e.new.clone(),
                        new: e.old.clone(),
                    })
                    .collect();
                self.apply(sheet, &swapped, true);
                self.active = sheet;
                Step::Cells {
                    sheet,
                    edits: swapped,
                }
            }
            Step::Whole { book, active } => {
                let now = std::mem::replace(&mut self.wb, *book);
                let was = std::mem::replace(&mut self.active, active);
                self.calc.clear();
                Step::Whole {
                    book: Box::new(now),
                    active: was,
                }
            }
        }
    }

    /// Gives the clock to the calculation (`TODAY()` and `NOW()`) and recalculates them.
    pub fn tick(&mut self, now: f64) {
        self.calc.set_now(now);
    }
}

impl Default for Book {
    fn default() -> Book {
        Book::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_recalculate_and_undo() {
        let mut b = Book::new();
        b.set(0, 0, "2");
        b.set(0, 1, "=A1*10");
        assert_eq!(b.value(0, 1), Value::Num(20.0));
        b.set(0, 0, "5");
        assert_eq!(b.value(0, 1), Value::Num(50.0));
        assert!(b.undo());
        assert_eq!(b.value(0, 1), Value::Num(20.0));
        assert!(b.redo());
        assert_eq!(b.value(0, 1), Value::Num(50.0));
        assert!(b.dirty());
    }

    #[test]
    fn a_structural_change_is_undone_as_a_whole() {
        let mut b = Book::new();
        b.set(0, 0, "x");
        b.structural(|wb, _| {
            wb.sheets.push(ultar_office::sheet::Sheet::new("Two"));
        });
        assert_eq!(b.wb.sheets.len(), 2);
        assert!(b.undo());
        assert_eq!(b.wb.sheets.len(), 1);
        assert!(b.redo());
        assert_eq!(b.wb.sheets.len(), 2);
    }

    #[test]
    fn formats_apply_to_the_shown_value() {
        let mut b = Book::new();
        b.set(0, 0, "0.256");
        b.restyle((0, 0, 0, 0), &|s| s.num_fmt = "0.0%".into());
        assert_eq!(b.shown(0, 0).text, "25.6%");
        assert!(b.undo());
        assert_eq!(b.shown(0, 0).text, "0.256");
    }
}
